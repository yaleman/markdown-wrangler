# Design Review Rubric

Use this rubric to score each page on a 0-3 scale per category:
- `0`: Fails badly, blocks core usage.
- `1`: Noticeable problems, degrades UX or trust.
- `2`: Minor issues, acceptable for most users.
- `3`: Strong quality, no notable issues.

## Categories

1. Visual Hierarchy
- Heading structure is clear and scannable.
- Primary actions are visually dominant.
- Content grouping is obvious.

2. Layout And Spacing
- Grid alignment is consistent.
- Spacing rhythm is consistent across sections/components.
- No overlap, clipping, or awkward whitespace at tested viewports.

3. Typography And Readability
- Font sizes and weights support hierarchy.
- Body text remains readable at mobile and desktop sizes.
- Line length and line-height are comfortable.

4. Color And Contrast
- Foreground/background contrast is legible.
- Color usage is consistent and semantic.
- Interactive states are visually distinct.

5. Responsiveness
- Layout adapts cleanly at all requested breakpoints.
- Navigation and key CTAs remain reachable and usable.
- No horizontal scrolling unless intentionally required.

6. Interaction Quality
- Buttons/links visibly react to hover/focus/press when appropriate.
- Form validation and error messages are clear.
- Loading, empty, and error states are present and understandable.

7. Accessibility Basics
- Focus indicator is visible.
- Keyboard traversal reaches primary controls in logical order.
- Interactive elements have understandable labels/text.

## Scoring And Outcome

- Sum category scores for a total out of 21.
- Suggested interpretation:
- `18-21`: Release-ready polish.
- `14-17`: Good baseline; fix medium issues before release.
- `10-13`: Quality risks; fix major issues before release.
- `<10`: Significant UX/design debt; rework recommended.

## Finding Format

For each issue, capture:
- Severity: `critical`, `major`, `minor`, or `nit`.
- Page/route and viewport.
- Reproduction steps.
- Expected behavior and actual behavior.
- Screenshot or DOM evidence.
- Suggested fix.
