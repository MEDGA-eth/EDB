/**
 * Centralised numeric / string constants for the web UI.
 *
 * Hoist values here when they:
 *  - need to be referenced from more than one module, or
 *  - are tunable parameters that benefit from a single, named, documented home.
 */

/** How often the healthcheck poll fires (ms). */
export const HEALTHCHECK_INTERVAL_MS = 2000;

/** Consecutive missed probes before the session is marked ended. */
export const HEALTHCHECK_FAILURE_THRESHOLD = 3;

/** Per-probe abort threshold (ms). Beyond this we treat the probe as a miss. */
export const HEALTHCHECK_TIMEOUT_MS = 1500;

/** Maximum number of "recently used" command-palette entries to remember. */
export const PALETTE_RECENT_LIMIT = 10;

/** Hard cap on rendered command-palette result rows. */
export const PALETTE_RESULT_CAP = 60;

/**
 * Bumped any time the persisted dockview-layout shape changes incompatibly.
 * On restore, mismatches drop the saved layout and re-seed defaults.
 *
 * History:
 *   1: initial bottom-panel layout (Trace + Display + Terminal tabs).
 *   2: Trace moved to the sidebar; bottom defaults to Display + Terminal only.
 *   3: Editor and bottom area unified into a single dockview so tabs can be
 *      dragged anywhere (e.g. terminal next to code). Persisted layouts
 *      from versions 1-2 are dropped on load and re-seeded.
 */
export const LAYOUT_VERSION = 3;

/** Width of the activity bar (icon column on the left), in CSS pixels. */
export const ACTIVITY_WIDTH_PX = 64;

/** Width of the explorer / breakpoints sidebar, in CSS pixels. */
export const SIDEBAR_WIDTH_PX = 280;

/** localStorage key for the persisted dockview layout JSON. */
export const LAYOUT_KEY = 'edb-web:layout';
