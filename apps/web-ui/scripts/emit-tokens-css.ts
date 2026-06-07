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

for (const [category, items] of Object.entries(tokens)) {
  if (category.startsWith('$') || typeof items !== 'object' || items === null) {
    continue;
  }
  lines.push(`  /* ${category} */`);
  for (const [key, def] of Object.entries(items)) {
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
