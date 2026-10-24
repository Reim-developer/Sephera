package com.acme;

/**
 * The Java half of the JNI fixture: two native methods whose bodies live
 * in C, and one ordinary method that stays in Java.
 */
public class Native {
    public static native int add(int a, int b);

    public static native void reset();

    public int ordinary(int a) {
        return a;
    }
}
