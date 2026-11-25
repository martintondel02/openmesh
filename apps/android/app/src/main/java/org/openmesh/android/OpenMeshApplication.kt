package org.openmesh.android

import android.app.Application
import org.openmesh.android.data.repository.MeshGraph

/** Owns the process-wide repository instance (see [MeshGraph]). */
class OpenMeshApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        MeshGraph.initialize(this)
    }
}
