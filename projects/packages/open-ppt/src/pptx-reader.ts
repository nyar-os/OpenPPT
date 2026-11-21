/**
 * Playback reader for a `.pptx`-family package.
 *
 * A reader holds one native playback handle and paints visible slides on
 * demand. It is created through `OpenPPT.readPptx()` so the owning session can
 * release it in `dispose()`.
 */

import { OpenPPTError } from './errors.js';
import * as service from './service.js';

export class PptxReader {
    /** Show summary resolved when the package was opened. */
    readonly presentation: service.PptxPresentation;
    private readonly handle: number;
    private readonly onClose: () => void;
    private done = false;

    /** @internal Created by `OpenPPT.readPptx()`. */
    constructor(opened: service.PptxHandle, onClose: () => void = () => {}) {
        this.presentation = opened.presentation;
        this.handle = opened.handle;
        this.onClose = onClose;
    }

    /** Whether {@link close} has run. */
    get closed(): boolean {
        return this.done;
    }

    /** Paints one visible slide by its one-based number. */
    slide(index: number): service.PptxSlideFrame {
        if (this.done) {
            throw new OpenPPTError('session/disposed', 'this presentation reader has been closed');
        }
        return service.pptxSlide(this.handle, index);
    }

    /** Releases the playback handle. Idempotent. */
    close(): void {
        if (this.done) {
            return;
        }
        this.done = true;
        service.closePptx(this.handle);
        this.onClose();
    }
}
