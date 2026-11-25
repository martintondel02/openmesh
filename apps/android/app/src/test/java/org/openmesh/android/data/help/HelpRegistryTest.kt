package org.openmesh.android.data.help

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class HelpRegistryTest {
    private val repoRoot: File = run {
        val candidates = PresetPath.candidates()
        candidates.map(::File).firstOrNull { File(it, "content/help/index.json").isFile }
            ?: error("content/help/index.json not found from ${File(".").absolutePath}")
    }
    private val slugs: Set<String> by lazy {
        val text = File(repoRoot, "content/help/index.json").readText()
        Regex("\"slug\"\\s*:\\s*\"([a-z0-9-]+)\"").findAll(text).map { it.groupValues[1] }.toSet()
    }

    @Test
    fun `every help key maps to a bundled topic slug`() {
        assertTrue(HelpKeys.registry.size >= 20)
        HelpKeys.registry.forEach { (control, slug) ->
            assertTrue("$control -> missing help topic $slug", slug in slugs)
        }
    }

    @Test
    fun `common controls have explanations`() {
        listOf(
            "settings.mode", "settings.radio.profile", "settings.radio.region",
            "settings.notifications", "chat.media", "channels.create", "contacts.scan",
            "radio.connect", "settings.backbone.tip", "settings.agents.tip",
        ).forEach { assertNotNull("missing help key $it", HelpKeys.slugFor(it)) }
    }

    @Test
    fun `unknown controls return null instead of a wrong topic`() {
        assertEquals(null, HelpKeys.slugFor("does.not.exist"))
    }
}

/** Small shared resolver for JVM tests that need the repository root. */
object PresetPath {
    fun candidates(): List<String> {
        val cwd = File(System.getProperty("user.dir"))
        val prop = System.getProperty("openmesh.repoRoot")
        return buildList {
            if (prop != null) add(prop)
            add(File(cwd, "../..").canonicalPath)
            add(File(cwd, "../../..").canonicalPath)
            add(cwd.canonicalPath)
        }
    }
}
