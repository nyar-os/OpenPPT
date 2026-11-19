/**
 * Presentation format family.
 *
 * PowerPoint extensions encode two things at once: the container (`x` = OOXML,
 * none = 97-2003 binary, `m` = macro-enabled) and the intended use
 * (`pps*` = show, `pot*` = template, `thmx` = theme). The registry below
 * classifies extensions and fixes the default open intent.
 *
 * Classification is **not** a support claim. What the installed engine can read,
 * write or play is reported by `OpenPPT.capabilities()`; an extension listed
 * here may still be `unsupported`.
 */

/** Deck intent applied when the caller does not override it. */
export type DeckIntent = 'edit' | 'play' | 'template';

/** Grouping used for capability reporting and user-facing wording. */
export type FormatFamily = 'core' | 'show' | 'template' | 'theme' | 'addin' | 'legacy' | 'document' | 'video' | 'image';

/** Container encoding behind an extension. */
export type FormatMarkup = 'ooxml' | 'binary' | 'odf' | 'xml' | 'fixed' | 'video' | 'raster';

/** What an extension can be consumed as. `none` means it never becomes a deck. */
export type FormatOpenKind = 'deck' | 'theme' | 'none';

/** Every extension the product must classify. */
export const presentationFormats = [
    'pptx',
    'pptm',
    'ppsx',
    'ppsm',
    'potx',
    'potm',
    'thmx',
    'ppam',
    'ppa',
    'ppt',
    'pps',
    'pot',
    'xml',
    'odp',
    'pdf',
    'xps',
    'mp4',
    'wmv',
    'png',
    'jpg',
    'jpeg',
    'gif',
    'tif',
    'tiff',
] as const;

/** Union of {@link presentationFormats}. */
export type PresentationFormat = (typeof presentationFormats)[number];

/** What a format is, independent of whether it is implemented. */
export interface PresentationFormatInfo {
    readonly format: PresentationFormat;
    readonly family: FormatFamily;
    readonly markup: FormatMarkup;
    /** Whether the container may hold VBA macros. Such parts are never executed. */
    readonly macroEnabled: boolean;
    readonly opensAs: FormatOpenKind;
    /** Intent applied to `open()` when the extension arrives without one. */
    readonly defaultIntent: DeckIntent | null;
}

type FormatRegistryEntry = Omit<PresentationFormatInfo, 'format'>;

/** Extension to classification. Values describe the format, never a capability. */
export const formatRegistry: Readonly<Record<PresentationFormat, FormatRegistryEntry>> = Object.freeze({
    pptx: { family: 'core', markup: 'ooxml', macroEnabled: false, opensAs: 'deck', defaultIntent: 'edit' },
    pptm: { family: 'core', markup: 'ooxml', macroEnabled: true, opensAs: 'deck', defaultIntent: 'edit' },
    ppsx: { family: 'show', markup: 'ooxml', macroEnabled: false, opensAs: 'deck', defaultIntent: 'play' },
    ppsm: { family: 'show', markup: 'ooxml', macroEnabled: true, opensAs: 'deck', defaultIntent: 'play' },
    potx: { family: 'template', markup: 'ooxml', macroEnabled: false, opensAs: 'deck', defaultIntent: 'template' },
    potm: { family: 'template', markup: 'ooxml', macroEnabled: true, opensAs: 'deck', defaultIntent: 'template' },
    thmx: { family: 'theme', markup: 'ooxml', macroEnabled: false, opensAs: 'theme', defaultIntent: null },
    ppam: { family: 'addin', markup: 'binary', macroEnabled: true, opensAs: 'none', defaultIntent: null },
    ppa: { family: 'addin', markup: 'binary', macroEnabled: true, opensAs: 'none', defaultIntent: null },
    ppt: { family: 'legacy', markup: 'binary', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    pps: { family: 'legacy', markup: 'binary', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    pot: { family: 'legacy', markup: 'binary', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    xml: { family: 'legacy', markup: 'xml', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    odp: { family: 'document', markup: 'odf', macroEnabled: false, opensAs: 'deck', defaultIntent: 'edit' },
    pdf: { family: 'document', markup: 'fixed', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    xps: { family: 'document', markup: 'fixed', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    mp4: { family: 'video', markup: 'video', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    wmv: { family: 'video', markup: 'video', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    png: { family: 'image', markup: 'raster', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    jpg: { family: 'image', markup: 'raster', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    jpeg: { family: 'image', markup: 'raster', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    gif: { family: 'image', markup: 'raster', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    tif: { family: 'image', markup: 'raster', macroEnabled: false, opensAs: 'none', defaultIntent: null },
    tiff: { family: 'image', markup: 'raster', macroEnabled: false, opensAs: 'none', defaultIntent: null },
});

/** Returns the full classification of one format. */
export function formatInfo(format: PresentationFormat): PresentationFormatInfo {
    return { format, ...formatRegistry[format] };
}

/** Extracts a classified extension from a path, ignoring case. */
export function formatFromPath(path: string): PresentationFormat | undefined {
    const extension = path.slice(path.lastIndexOf('.') + 1).toLowerCase();
    return (presentationFormats as readonly string[]).includes(extension) ? (extension as PresentationFormat) : undefined;
}

/** Default open intent of a format, or `null` when it never opens as a deck. */
export function defaultIntentFor(format: PresentationFormat): DeckIntent | null {
    return formatRegistry[format].defaultIntent;
}
