import { spawnSync } from "node:child_process";

/**
 * Touchstone's acceptance gate, as a weave skill.
 *
 * Deterministic on purpose: this is a CODE skill, not an agent skill, so the hourly run costs no
 * tokens and cannot hallucinate a green result. The gate itself (tests/verify.sh) owns what "correct"
 * means -- build, tests, hex analyze, the conformance suite over every bundle, and byte-exact export.
 * This skill just runs it and reports.
 *
 * Failure summaries carry the failing lines, because "the gate failed" in a notification is useless
 * at 3am and "acme_retail: index.md set changed" tells you what happened.
 */
const REPO = process.env.TOUCHSTONE_REPO ?? "/Volumes/SSD/Development/touchstone";

export default {
  name: "touchstone-verify",
  description:
    "Run Touchstone's acceptance gate: build, tests, hex analyze, the touchstone-conformance drills " +
    "across all bundles, and byte-exact export. Deterministic, no LLM.",
  match: (t) => /touchstone.*(verify|gate|check)|verify.*touchstone/i.test(t.spec.goal),

  async run() {
    const started = Date.now();
    const r = spawnSync("bash", ["tests/verify.sh"], {
      cwd: REPO,
      encoding: "utf8",
      timeout: 15 * 60_000,
      env: { ...process.env },
    });

    // Strip ANSI so the summary is readable in a notification and in the event log.
    const out = `${r.stdout ?? ""}${r.stderr ?? ""}`.replace(/\[[0-9;]*m/g, "");
    const tally = out.match(/^\s*(\d+) passed, (\d+) failed/m);
    const secs = Math.round((Date.now() - started) / 1000);

    if (r.error) {
      return { status: "failed", summary: `gate could not run: ${r.error.message}` };
    }

    if (r.status === 0) {
      return {
        status: "completed",
        summary: `Touchstone gate green — ${tally ? tally[1] : "?"} checks passed in ${secs}s`,
      };
    }

    const failures = out
      .split("\n")
      .filter((l) => l.includes("FAIL"))
      .map((l) => l.trim())
      .slice(0, 8);

    return {
      status: "failed",
      summary:
        `Touchstone gate FAILED (${tally ? `${tally[2]} of ${Number(tally[1]) + Number(tally[2])}` : "?"}) after ${secs}s\n` +
        failures.join("\n"),
    };
  },
};
