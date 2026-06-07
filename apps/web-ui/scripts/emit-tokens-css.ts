// Generate `tokens.css` from `tokens.json` per ADR-014 §C. Runs via
// `bun run emit-tokens` from the repo root. CI's drift-check step
// (.github/workflows/web-ci.yml) re-runs this and fails if the
// committed `tokens.css` diverges — same regression-tested-floor model
// as the openapi schema-drift check on the Rust side.
//
// Layout: walks each top-level category (color, status, r, shadow),
// emits one CSS custom property per leaf as `--{category}-{key}`. The
// `tenantOverride: true` metadata is intentionally ignored here — the
// emitted CSS contains the fallback values; runtime tenant overrides
// inject a separate `:root { ... }` block at boot per ADR-014 §F. A
// single section header comment per category groups the output for
// human readability.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

interface TokenDef {
  value: string;
  tenantOverride?: boolean;
}

type TokenCategory = Record<string, TokenDef>;

interface TokensFile {
  $comment?: string;
  [category: string]: TokenCategory | string | undefined;
}

const here = dirname(fileURLToPath(import.meta.url));
const tokensDir = join(here, '..', 'tokens');
const inputPath = join(tokensDir, 'tokens.json');
const outputPath = join(tokensDir, 'tokens.css');

const raw = readFileSync(inputPath, 'utf-8');
const tokens = JSON.parse(raw) as TokensFile;

const HEADER = [
  '/* ============================================================================',
  ' * GENERATED FILE — DO NOT EDIT',
  ' * ============================================================================',
  ' *',
  ' * Source of truth:  apps/web-ui/tokens/tokens.json',
  ' * Emitter:          apps/web-ui/scripts/emit-tokens-css.ts',
  ' * Regenerate:       bun run emit-tokens',
  ' * Contract:         ADR-014 §C',
  ' *',
  " * CI's tokens-drift step fails if this file diverges from what the emitter",
  ' * produces from the current tokens.json. Hand-edits will be reverted on the',
  ' * next emitter run; edit tokens.json instead.',
  ' * ============================================================================',
  ' */',
  '',
  '',
].join('\n');

const lines: string[] = [HEADER, ':root {'];

// Reject values whose content would break the surrounding `:root { … }`
// block. tokens.json is committed source today and reviewed via PR, so
// this is a typo-and-future-proofing guard rather than a security
// boundary — a `}` would end the declaration block early; a `;` belongs
// to the emitter, not the value. Real CSS values (oklch(), shadow
// triplets) use commas and parentheses, never these chars at the top
// level. Crash loud so the offender is caught at emit-time instead of
// at runtime cascade.
function assertValueShape(category: string, key: string, value: string): void {
  if (value.includes('}') || value.includes(';')) {
    throw new Error(
      `Token value for ${category}.${key} contains an illegal CSS structural character (\`}\` or \`;\`): ${value}`,
    );
  }
}

for (const [category, items] of Object.entries(tokens)) {
  if (category.startsWith('$') || typeof items !== 'object' || items === null) {
    continue;
  }
  lines.push(`  /* ${category} */`);
  for (const [key, def] of Object.entries(items)) {
    assertValueShape(category, key, def.value);
    lines.push(`  --${category}-${key}: ${def.value};`);
  }
  lines.push('');
}

// Trim the trailing blank line before the closing brace.
if (lines.at(-1) === '') {
  lines.pop();
}
lines.push('}');
lines.push('');

writeFileSync(outputPath, lines.join('\n'));
console.log(`Wrote ${outputPath}`);
