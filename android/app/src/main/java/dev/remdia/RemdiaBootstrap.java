package dev.remdia;

import android.content.Context;

/**
 * Same-package bootstrap: {@link Bridge#initialize(Context)} is package
 * private, and the app embeds the remdia engine in its own native library
 * instead of loading the AAR's libremedia_sdk.so. Call once before any
 * pordo native media or image work.
 */
public final class RemdiaBootstrap {
    private RemdiaBootstrap() {}

    public static void initialize(Context context) {
        Bridge.initialize(context);
    }
}
