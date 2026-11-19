/**
 * Authoring surface of `@wae/open-ppt`.
 *
 * Everything here is a declared contract: the session, deck handle and format
 * registry exist, while every capability rejects with
 * `capability/not-implemented` until the shared engine lands. Domain rules,
 * layout, rendering and PPTX exchange belong upstream and are never
 * re-implemented in this package.
 */

export { Deck } from './deck.js';
export { OpenPPT } from './open-ppt.js';
export { closePptx, openPptx, pptxSlide } from './service.js';
export type { PptxHandle, PptxIssue, PptxPresentation, PptxSlideFrame } from './service.js';
export { GuiError, closeGui, defaultGuiContentDir, isGuiOpen, openGui, pumpEvents, runGui } from './gui.js';
export type { GuiCloseResult, GuiOpenResult, GuiStatus, OpenGuiOptions } from './gui.js';
export { NotImplementedError, OpenPPTError, errorCodes, isOpenPPTError } from './errors.js';
export type { OpenPPTErrorCode, OpenPPTErrorDetails, OpenPPTErrorOptions } from './errors.js';
export { defaultIntentFor, formatFromPath, formatInfo, formatRegistry, presentationFormats } from './formats.js';
export type {
    DeckIntent,
    FormatFamily,
    FormatMarkup,
    FormatOpenKind,
    PresentationFormat,
    PresentationFormatInfo,
} from './formats.js';
export type * from './types.js';
