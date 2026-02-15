---
name: playwright
description: Validate web page visual quality, aesthetic consistency, and functional UX behavior using Playwright browser automation. Use when asked to review UI design, test responsive layouts/interactions/forms/navigation, catch regressions, or produce evidence-backed design QA findings with screenshots and reproducible steps.
---

# Playwright Design Validator

## Overview

Use Playwright to inspect real rendered pages and validate visual, aesthetic, and functional quality.
Produce actionable findings with severity, reproduction steps, and screenshot evidence.

Use `pnpm` commands only for this skill.
Never use `npx` for Playwright execution in this repository.

Install browser dependencies with:
`pnpm playwright install chrome-for-testing`

Write screenshots and other generated evidence to:
`/tmp/pw-audit/`

## Inputs

Collect these inputs before testing:
- Target URL or route list.
- Intended viewport set (desktop/tablet/mobile). Default to `1440x900`, `1024x768`, `390x844`.
- User journey scope (for example: landing page, signup flow, settings form).
- Known design constraints (design system, brand rules, components that are intentionally unfinished).

## Execution Workflow

1. Open the page in Playwright and wait for network and UI stabilization.
2. Capture baseline evidence:
- Full-page screenshot.
- Above-the-fold screenshot.
- Optional component-focused screenshots for dense UIs.
 - Save all captures under `/tmp/pw-audit/` with clear names including page + viewport.
3. Validate visual and aesthetic quality:
- Check layout alignment, spacing rhythm, visual hierarchy, and typography consistency.
- Check color/contrast consistency with expected design direction.
- Check responsive behavior at each required viewport.
4. Validate functional behavior:
- Test primary navigation, core buttons/links, and key forms.
- Test positive and negative states (validation errors, empty states, loading states, error states).
- Test keyboard behavior (Tab order, visible focus, Enter/Space activation where relevant).
5. Record findings as evidence-first:
- Severity (`critical`, `major`, `minor`, `nit`).
- Concrete location (`page`, `component`, selector/text anchor if known).
- Reproduction steps and expected vs actual behavior.
- Screenshot reference.
6. Propose fixes:
- Keep fixes implementation-oriented and scoped.
- Prioritize by user impact and frequency.

## Reporting Rules

- Report findings first, ordered by severity, then by user impact.
- Avoid generic statements like "looks off"; describe measurable issues.
- Separate aesthetic preference from objective usability/accessibility risk.
- If no issues are found, state that explicitly and list what was tested.

## Command Conventions

- Use `pnpm playwright ...` for screenshot, pdf, or test commands.
- Set `TMPDIR=/tmp` when running Playwright commands in restricted environments.
- Use `/tmp/pw-audit` output paths, for example:
`mkdir -p /tmp/pw-audit`
`TMPDIR=/tmp pnpm playwright screenshot --device="Desktop Chrome" http://127.0.0.1:5420 /tmp/pw-audit/index-desktop-fold.png`
`TMPDIR=/tmp pnpm playwright screenshot --device="Desktop Chrome" --full-page http://127.0.0.1:5420 /tmp/pw-audit/index-desktop-full.png`

## Output Template

Use this structure in responses:

1. Scope: URLs/routes, viewports, and journeys tested.
2. Findings: Severity-sorted issues with repro steps and evidence.
3. Functional Coverage: What interactions/states were validated.
4. Residual Risk: What was not tested and why.
5. Fix Priorities: Top recommended fixes in execution order.

## Reference Material

Load `references/design-review-rubric.md` when:
- Needing a numeric scorecard.
- Comparing multiple pages or revisions.
- Requiring standardized pass/fail criteria.
