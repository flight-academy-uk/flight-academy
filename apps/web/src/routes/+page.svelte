<!--
  Placeholder landing. Demonstrates the design-token + base-style chain
  end-to-end:
    * Body background + ink colour come from tokens via base.css.
    * Headings use the `flight-academy-*` typography utilities.
    * Dark mode auto-respects the OS via `prefers-color-scheme`; the
      tiny toggle below flips `data-theme` on `<html>` to override.
  Real v1 design ports land in C6+ once C3 (primitives) and C4 (generated
  TS client wrap) are in place.
-->

<script lang="ts">
  // Cycle through: auto (no data-theme) → 'light' → 'dark' → auto.
  // Read-once at render so SSR + hydration agree; subsequent clicks
  // mutate the attribute directly.
  let theme: 'auto' | 'light' | 'dark' = $state('auto');

  function cycleTheme() {
    theme = theme === 'auto' ? 'light' : theme === 'light' ? 'dark' : 'auto';
    const root = document.documentElement;
    if (theme === 'auto') {
      root.removeAttribute('data-theme');
    } else {
      root.setAttribute('data-theme', theme);
    }
  }
</script>

<main class="flex min-h-screen items-center justify-center">
  <div class="text-center">
    <h1 class="flight-academy-h1">Flight Academy</h1>
    <p class="flight-academy-muted mt-2">Web skeleton — design tokens + base styles wired.</p>
    <button
      class="flight-academy-small mt-6 cursor-pointer rounded border border-current px-3 py-1"
      onclick={cycleTheme}
    >
      theme: {theme}
    </button>
  </div>
</main>
