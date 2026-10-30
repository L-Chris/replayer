package dev.replayer

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Offline UI flow driven by semantic test tags rather than coordinates, so it
 * survives layout changes: the startup gate stays hidden, the comics tab is
 * reachable and a blank keyword never starts a search.
 */
@RunWith(AndroidJUnit4::class)
class NavigationUiTest {
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()

    @Test
    fun komiicAccountSettingsAreReachable() {
        compose.onNodeWithTag("nav-comics").performClick()
        compose.onNodeWithTag("nav-comic-source").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("komiic-login").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("komiic-email").assertIsDisplayed()
        compose.onNodeWithTag("komiic-password").assertIsDisplayed()
        compose.onNodeWithTag("komiic-status").assertExists()
    }

    @Test
    fun cloudSettingsAreReachable() {
        compose.onNodeWithTag("nav-cloud").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("sync-save").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("sync-enabled").assertIsDisplayed()
        compose.onNodeWithTag("sync-status").assertExists()
    }

    @Test
    fun startsReadyWithoutTheInitializationGate() {
        assertTrue(Native.initError ?: "bridge not ready", Native.ready)
        compose.onNodeWithTag("init-error-title").assertDoesNotExist()
        compose.onNodeWithTag("nav-comics").assertIsDisplayed()
    }

    @Test
    fun comicsTabReachesTheSearchScreen() {
        openSearch()
        compose.onNodeWithTag("comic-search-field").assertIsDisplayed()
        compose.onNodeWithTag("comic-search-hint").assertIsDisplayed()
    }

    @Test
    fun blankKeywordNeverStartsASearch() {
        openSearch()
        compose.onNodeWithTag("comic-search-field").performTextInput("   ")
        compose.onNodeWithTag("comic-search-button").performClick()
        compose.waitUntil(2_000) { compose.onAllNodesWithTag("comic-search-error").fetchSemanticsNodes().isEmpty() }
        compose.onNodeWithTag("comic-search-hint").assertIsDisplayed()
        compose.onNodeWithTag("comic-search-error").assertDoesNotExist()
    }

    private fun openSearch() {
        compose.onNodeWithTag("nav-comics").assertIsDisplayed().performClick()
        compose.onNodeWithTag("comic-tab-search").assertIsDisplayed().performClick()
    }
}
