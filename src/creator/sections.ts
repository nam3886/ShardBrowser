// Our pages' section ids, spread into upstream's SECTIONS (src/shared/types).
// No imports here: shared/types imports this file.
export const CREATOR_SECTIONS = ["c-accounts", "c-emails", "c-proxies", "c-platforms"] as const;
export type CreatorSection = (typeof CREATOR_SECTIONS)[number];
