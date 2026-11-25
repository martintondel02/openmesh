package org.openmesh.android.util

import android.util.Log

/** Single logging facade for field support; tag is always "OpenMesh". */
object MeshLog {
    const val TAG = "OpenMesh"

    fun i(message: String, throwable: Throwable? = null) {
        if (throwable != null) Log.i(TAG, message, throwable) else Log.i(TAG, message)
    }

    fun w(message: String, throwable: Throwable? = null) {
        if (throwable != null) Log.w(TAG, message, throwable) else Log.w(TAG, message)
    }

    fun e(message: String, throwable: Throwable? = null) {
        if (throwable != null) Log.e(TAG, message, throwable) else Log.e(TAG, message)
    }
}
