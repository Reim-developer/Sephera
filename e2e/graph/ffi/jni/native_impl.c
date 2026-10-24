/* The C half of the JNI fixture: the implementations of Native's native
 * methods, named by the JNI mangling rule and nothing else. No file in
 * this tree includes or imports the other half; the mangled names are
 * the whole connection. */
#include <jni.h>

JNIEXPORT jint JNICALL Java_com_acme_Native_add(JNIEnv *env, jclass cls, jint a, jint b) {
    return a + b;
}

JNIEXPORT void JNICALL Java_com_acme_Native_reset(JNIEnv *env, jclass cls) {
}
