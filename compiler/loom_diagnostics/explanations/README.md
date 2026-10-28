# Loom Diagnostic Explanations

This directory contains Markdown explanations for Loom compiler error codes (e.g., `loom explain E0007`).

## File Format

Each file is named `EXXXX.md` and uses the following structure:

```markdown
# EXXXX: <Error title>

One or two short paragraphs explaining what the error means and why Loom has this rule.

## Example

```loom
<smallest program that produces this error>
```

## How to fix it

One sentence, followed by:

```loom
<the corrected program>
```

See spec §X.Y.
```
