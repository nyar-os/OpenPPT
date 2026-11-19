/**
 * `new OpenPPT(options?)` authoring session.
 *
 * The constructor is synchronous and opens nothing: no network, no disk scan,
 * no window, no file. Native initialisation and imports happen inside the async
 * methods, so a failure is always awaited and always carries a stable code.
 */

import { NotImplementedError, OpenPPTError } from './errors.js';
import { Deck } from './deck.js';
import * as service from './service.js';
import type {
    DeckCreateOptions,
    DeckInput,
    InspectOptions,
    InspectReport,
    OpenPPTCapabilities,
    OpenPPTOptions,
} from './types.js';

/** Authoring session. One instance may manage several decks. */
export class OpenPPT {
    /** BCP-47 tag used for diagnostics and default copy. */
    readonly locale: string;
    private closed = false;

    constructor(options: OpenPPTOptions = {}) {
        this.locale = options.locale ?? 'en';
    }

    /** Whether {@link dispose} has run. */
    get disposed(): boolean {
        return this.closed;
    }

    /**
     * Format, checker, renderer and playback disclosure for the installed
     * engine. Read this instead of treating a method name as a capability.
     */
    async capabilities(): Promise<OpenPPTCapabilities> {
        this.assertActive();
        return service.capabilities();
    }

    /** Creates an empty deck with its own handle. */
    async create(_options?: DeckCreateOptions): Promise<Deck> {
        this.assertActive();
        throw new NotImplementedError('create');
    }

    /** Opens a native project, `.pptx`-family file or in-memory package. */
    async open(input: DeckInput): Promise<Deck> {
        this.assertActive();
        assertDeckInput(input);
        throw new NotImplementedError('open');
    }

    /** Inspects an input without keeping a deck handle. */
    async inspect(input: DeckInput, _options?: InspectOptions): Promise<InspectReport> {
        this.assertActive();
        assertDeckInput(input);
        throw new NotImplementedError('inspect');
    }

    /** Releases the session and every handle it owns. Idempotent. */
    async dispose(): Promise<void> {
        this.closed = true;
    }

    private assertActive(): void {
        if (this.closed) {
            throw new OpenPPTError('session/disposed', 'the OpenPPT session has been disposed; create a new instance');
        }
    }
}

function assertDeckInput(input: DeckInput): void {
    const candidate = input as unknown as Record<string, unknown>;
    const present = (['path', 'bytes', 'project'] as const).filter((key) => candidate[key] !== undefined);
    if (present.length !== 1) {
        throw new OpenPPTError(
            'session/argument-invalid',
            'DeckInput must contain exactly one of `path`, `bytes` or `project`',
        );
    }
    const value = candidate[present[0]];
    if (present[0] === 'bytes') {
        if (!(value instanceof Uint8Array)) {
            throw new OpenPPTError('session/argument-invalid', '`bytes` must be a Uint8Array');
        }
        return;
    }
    if (typeof value !== 'string' || value.length === 0) {
        throw new OpenPPTError('session/argument-invalid', `\`${present[0]}\` must be a non-empty string`);
    }
}
