// Whole app is prerendered for adapter-static (ADR-014 §A). Sensitive
// routes that need SSR (login, magic-link verify, passkey ceremony,
// OAuth consent — ADR-015 §C) will set prerender = false at their own
// route level when those land.

export const prerender = true;
