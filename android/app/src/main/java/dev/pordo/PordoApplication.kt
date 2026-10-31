package dev.pordo

import android.app.Application

/**
 * Loads the native bridge once per process, before any activity or service.
 * A failure is recorded in [Native.initError] rather than thrown: MainActivity
 * gates on [Native.ready] and shows a diagnosable, retryable error state, so
 * broken installs never crash before the first frame and no screen can issue a
 * JNI call that would abort the process.
 */
class PordoApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        Native.initialize(this)
        dev.pordo.sync.CloudSync.initialize(this)
    }
}
