// Our words that upstream modules show (no React here: plain modules import it).
import { ct } from "./i18n";

/** Whether our Settings tab shows (upstream's command registry asks). */
export { CREATOR_DATA } from "./api";

/** Our Settings tab's name: the tab list, its heading and the palette's "Settings: …" command. */
export const creatorTabLabel = () => ct("creator.settings.tab");
