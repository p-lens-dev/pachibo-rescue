package pachibo;

import java.io.BufferedReader;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;

/**
 * The screen as uiautomator describes it, dumped again and again by one
 * process, rather than by a new `uiautomator dump` each time (whose process
 * takes seconds to start on some phones).
 *
 * Run on the phone by app_process with the phone's own uiautomator.jar on the
 * class path, which does the work exactly as `uiautomator dump --compressed`
 * does: connect, wait for the screen to be idle, dump the active window. It
 * is reached by reflection, so this builds with a JDK alone.
 *
 * Talks over standard input and output, a line at a time:
 *   on start, "READY 1" — or "FAIL <why>", and it exits;
 *   "dump"  → "OK <bytes>" then that many bytes of XML, or "ERR <why>";
 *   "settext <hex>" → the view with input focus (else the first that takes
 *     text) is given that text (UTF-8,
 *     in hex), as accessibility sets it, past the keyboard (which would
 *     convert it) → "OK 0", or "ERR <why>". For a tool that fills in screens;
 *     the rescue itself only reads;
 *   "quit", or the end of input → it disconnects and exits.
 * A dump that fails (uiautomator can throw on a screen that changes as it is
 * read) fails alone: the process goes on to the next.
 */
public final class Dumper {
    /** The protocol's version, said on start. */
    private static final int VERSION = 2;
    /** AccessibilityNodeInfo.FOCUS_INPUT. */
    private static final int FOCUS_INPUT = 1;
    /** AccessibilityNodeInfo.ACTION_SET_TEXT. */
    private static final int ACTION_SET_TEXT = 0x00200000;
    /** AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE. */
    private static final String SET_TEXT_ARGUMENT = "ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE";
    /**
     * Quiet time that counts as the screen being idle: half a second, as the
     * uiautomator test framework itself waits (QueryController), where
     * `uiautomator dump` waits a whole second. A read taken mid-change is
     * caught all the same by the walk, which checks every screen it reads.
     */
    private static final long IDLE_MILLIS = 500;
    /** The longest wait for that quiet, as uiautomator dump waits. */
    private static final long IDLE_LIMIT_MILLIS = 10000;

    private Dumper() {}

    public static void main(String[] args) throws Exception {
        OutputStream out = System.out;
        BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        File dumpFile = new File(args.length > 0 ? args[0] : "/data/local/tmp/pachibo-dumper.xml");

        Object wrapper;
        Object automation;
        try {
            Class<?> wrapperClass = Class.forName("com.android.uiautomator.core.UiAutomationShellWrapper");
            wrapper = wrapperClass.getConstructor().newInstance();
            wrapperClass.getMethod("connect").invoke(wrapper);
            setCompressed(wrapperClass, wrapper);
            automation = wrapperClass.getMethod("getUiAutomation").invoke(wrapper);
        } catch (Throwable error) {
            line(out, "FAIL " + describe(error));
            return;
        }
        line(out, "READY " + VERSION);

        try {
            String command;
            while ((command = in.readLine()) != null) {
                command = command.trim();
                if (command.equals("quit")) {
                    break;
                } else if (command.equals("dump")) {
                    try {
                        byte[] xml = dump(automation, dumpFile);
                        line(out, "OK " + xml.length);
                        out.write(xml);
                        out.flush();
                    } catch (Throwable error) {
                        line(out, "ERR " + describe(error));
                    }
                } else if (command.startsWith("settext ")) {
                    try {
                        setText(automation, new String(fromHex(command.substring("settext ".length())), StandardCharsets.UTF_8));
                        line(out, "OK 0");
                    } catch (Throwable error) {
                        line(out, "ERR " + describe(error));
                    }
                } else if (!command.isEmpty()) {
                    line(out, "ERR unknown command: " + command);
                }
            }
        } finally {
            try {
                wrapper.getClass().getMethod("disconnect").invoke(wrapper);
            } catch (Throwable ignored) {
                // Leaving anyway.
            }
            dumpFile.delete();
        }
        // The uiautomation connection's threads would keep the process alive.
        System.exit(0);
    }

    /** Leave out the views that only lay others out, as `uiautomator dump --compressed` does. */
    private static void setCompressed(Class<?> wrapperClass, Object wrapper) throws Exception {
        for (String name : new String[] {"setCompressedLayoutHierarchy", "setCompressedLayoutHeirarchy"}) {
            try {
                wrapperClass.getMethod(name, boolean.class).invoke(wrapper, true);
                return;
            } catch (NoSuchMethodException tryNext) {
                // The other spelling.
            }
        }
        throw new NoSuchMethodException("setCompressedLayoutHierarchy");
    }

    /** One dump of the active window, as uiautomator's DumpCommand takes it. */
    private static byte[] dump(Object automation, File dumpFile) throws Throwable {
        Class<?> automationClass = automation.getClass();
        try {
            automationClass.getMethod("waitForIdle", long.class, long.class).invoke(automation, IDLE_MILLIS, IDLE_LIMIT_MILLIS);
        } catch (InvocationTargetException error) {
            throw new IllegalStateException("could not get idle state: " + describe(error.getCause()));
        }
        Object root = automationClass.getMethod("getRootInActiveWindow").invoke(automation);
        if (root == null) {
            throw new IllegalStateException("no active window");
        }

        Class<?> globalClass = Class.forName("android.hardware.display.DisplayManagerGlobal");
        Object global = globalClass.getMethod("getInstance").invoke(null);
        Object display = globalClass.getMethod("getRealDisplay", int.class).invoke(global, 0);
        int rotation = (Integer) display.getClass().getMethod("getRotation").invoke(display);
        Class<?> pointClass = Class.forName("android.graphics.Point");
        Object size = pointClass.getConstructor().newInstance();
        display.getClass().getMethod("getRealSize", pointClass).invoke(display, size);
        int width = pointClass.getField("x").getInt(size);
        int height = pointClass.getField("y").getInt(size);

        Class<?> nodeClass = Class.forName("android.view.accessibility.AccessibilityNodeInfo");
        Class<?> dumperClass = Class.forName("com.android.uiautomator.core.AccessibilityNodeInfoDumper");
        Method dumpWindow = dumperClass.getMethod("dumpWindowToFile", nodeClass, File.class, int.class, int.class, int.class);
        dumpFile.delete();
        try {
            dumpWindow.invoke(null, root, dumpFile, rotation, width, height);
        } catch (InvocationTargetException error) {
            throw error.getCause();
        }
        return readAll(dumpFile);
    }

    /** Give the view with input focus this text, and check it took. */
    private static void setText(Object automation, String text) throws Throwable {
        Object root = automation.getClass().getMethod("getRootInActiveWindow").invoke(automation);
        if (root == null) {
            throw new IllegalStateException("no active window");
        }
        Class<?> nodeClass = Class.forName("android.view.accessibility.AccessibilityNodeInfo");
        Object focused = nodeClass.getMethod("findFocus", int.class).invoke(root, FOCUS_INPUT);
        if (focused == null) {
            focused = firstEditable(nodeClass, root);
        }
        if (focused == null) {
            throw new IllegalStateException("no view takes text");
        }
        Class<?> bundleClass = Class.forName("android.os.Bundle");
        Object arguments = bundleClass.getConstructor().newInstance();
        bundleClass.getMethod("putCharSequence", String.class, CharSequence.class).invoke(arguments, SET_TEXT_ARGUMENT, text);
        boolean done = (Boolean) nodeClass.getMethod("performAction", int.class, bundleClass).invoke(focused, ACTION_SET_TEXT, arguments);
        if (!done) {
            throw new IllegalStateException("the view would not take the text");
        }
        nodeClass.getMethod("refresh").invoke(focused);
        Object now = nodeClass.getMethod("getText").invoke(focused);
        if (now == null || !now.toString().equals(text)) {
            throw new IllegalStateException("the view holds other text than was set");
        }
    }

    /** The first view under `node`, depth first, that takes text. */
    private static Object firstEditable(Class<?> nodeClass, Object node) throws Exception {
        if ((Boolean) nodeClass.getMethod("isEditable").invoke(node)) {
            return node;
        }
        int children = (Integer) nodeClass.getMethod("getChildCount").invoke(node);
        for (int at = 0; at < children; at++) {
            Object child = nodeClass.getMethod("getChild", int.class).invoke(node, at);
            if (child != null) {
                Object found = firstEditable(nodeClass, child);
                if (found != null) {
                    return found;
                }
            }
        }
        return null;
    }

    private static byte[] fromHex(String hex) {
        if (hex.length() % 2 != 0) {
            throw new IllegalArgumentException("odd hex");
        }
        byte[] bytes = new byte[hex.length() / 2];
        for (int at = 0; at < bytes.length; at++) {
            bytes[at] = (byte) Integer.parseInt(hex.substring(at * 2, at * 2 + 2), 16);
        }
        return bytes;
    }

    private static byte[] readAll(File file) throws Exception {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        InputStream in = new FileInputStream(file);
        try {
            byte[] buffer = new byte[65536];
            int read;
            while ((read = in.read(buffer)) > 0) {
                bytes.write(buffer, 0, read);
            }
        } finally {
            in.close();
        }
        return bytes.toByteArray();
    }

    private static void line(OutputStream out, String text) throws Exception {
        out.write((text + "\n").getBytes(StandardCharsets.UTF_8));
        out.flush();
    }

    /** An error in one line: its class, its message, and where it was thrown. */
    private static String describe(Throwable error) {
        Throwable cause = error;
        while (cause instanceof InvocationTargetException && cause.getCause() != null) {
            cause = cause.getCause();
        }
        StringWriter trace = new StringWriter();
        cause.printStackTrace(new PrintWriter(trace));
        String first = trace.toString().split("\n", 3).length > 1 ? trace.toString().split("\n", 3)[1].trim() : "";
        return (cause.getClass().getName() + ": " + cause.getMessage() + " " + first).replace('\n', ' ');
    }
}
