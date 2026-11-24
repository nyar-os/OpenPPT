/**
 * `new OpenPPT(options?)` authoring session.
 *
 * The constructor is synchronous and opens nothing: no network, no disk scan,
 * no window, no file. Native initialisation and imports happen inside the async
 * methods, so a failure is always awaited and always carries a stable code.
 */

import { OpenPPTError, setErrorLocale } from './errors.js';
import { Deck, type DeckOwner } from './deck.js';
import { formatFromPath, type DeckIntent, type PresentationFormat } from './formats.js';
import { PptxReader } from './pptx-reader.js';
import * as service from './service.js';
import type {
    CustomDeckSize,
    DeckCreateOptions,
    DeckInput,
    DeckSize,
    InspectOptions,
    InspectReport,
    ImportLoss,
    OpenPPTCapabilities,
    OpenPPTOptions,
} from './types.js';

/** Authoring session. One instance may manage several decks. */
export class OpenPPT implements DeckOwner {
    /** BCP-47 tag used for diagnostics and default copy. */
    readonly locale: string;
    private closed = false;
    private readonly decks = new Set<Deck>();
    private readonly readers = new Set<PptxReader>();

    constructor(options: OpenPPTOptions = {}) {
        this.locale = options.locale ?? 'en';
        setErrorLocale(this.locale);
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
    async create(options?: DeckCreateOptions): Promise<Deck> {
        this.assertActive();
        let theme: string | undefined;
        if (options?.theme !== undefined) {
            if (typeof options.theme !== 'string') {
                throw new OpenPPTError(
                    'object/unsupported',
                    'theme definitions need `deck.themes.apply()`; `create()` accepts a theme id',
                    { details: { feature: 'create.theme' } },
                );
            }
            theme = options.theme;
        }
        const request: service.CreateDeckRequest = {
            title: options?.title,
            size: options?.size === undefined ? undefined : createSize(options.size),
            theme,
            slides: options?.slides === undefined ? undefined : [...options.slides],
        };
        return this.attach(service.create(request));
    }

    /** Opens a native project, `.pptx`-family file or in-memory package. */
    async open(input: DeckInput): Promise<Deck> {
        this.assertActive();
        assertDeckInput(input);
        if (!('path' in input)) {
            const key = 'bytes' in input ? 'bytes' : 'project';
            throw new OpenPPTError(
                'format/unsupported',
                `opening \`${key}\` needs the package importer; write the project to a path and pass \`path\``,
                { details: { feature: `open.${key}` } },
            );
        }
        const intent: DeckIntent = input.intent ?? 'edit';
        if (intent !== 'edit') {
            throw new OpenPPTError(
                'format/unsupported',
                `native projects open for editing; \`${intent}\` needs the projection pipeline`,
                { details: { feature: `open.intent.${intent}` } },
            );
        }
        const opened = service.open(input.path);
        // A native project carries no exchange extension; report the projection
        // target until import metadata records the real source format.
        const format: PresentationFormat = formatFromPath(input.path) ?? 'pptx';
        return this.attach(opened, format, intent);
    }

    /** Inspects an input without keeping a deck handle. */
    async inspect(input: DeckInput, options?: InspectOptions): Promise<InspectReport> {
        this.assertActive();
        const deck = await this.open(input);
        try {
            return await deck.inspect(options);
        } finally {
            await deck.close();
        }
    }

    /**
     * Imports a `.pptx`-family package as an editable deck.
     *
     * Text, placeholders, pictures, preset shapes and tables are carried.
     * Refused constructs (charts, SmartArt, OLE objects, media, groups) are
     * reported on {@link Deck.importLosses} instead of being faked.
     */
    async importPptx(path: string): Promise<Deck> {
        this.assertActive();
        if (typeof path !== 'string' || path.trim().length === 0) {
            throw new OpenPPTError('session/argument-invalid', '`importPptx()` needs a non-empty package path');
        }
        const imported = service.importPptx(path);
        return this.attach(imported, 'pptx', 'edit', imported.losses);
    }

    /** Releases the session and every handle it owns. Idempotent. */
    async dispose(): Promise<void> {
        if (this.closed) {
            return;
        }
        this.closed = true;
        for (const reader of [...this.readers]) {
            reader.close();
        }
        this.readers.clear();
        for (const deck of [...this.decks]) {
            try {
                await deck.close();
            } catch {
                // A handle that already vanished must not keep the rest alive.
                deck.markDisposed();
            }
        }
        this.decks.clear();
    }

    /**
     * Opens a `.pptx`-family package for playback without building an editable
     * deck. The returned reader is closed by {@link dispose} if the caller
     * does not close it first.
     */
    async readPptx(path: string): Promise<PptxReader> {
        this.assertActive();
        if (typeof path !== 'string' || path.trim().length === 0) {
            throw new OpenPPTError('session/argument-invalid', '`readPptx()` needs a non-empty package path');
        }
        const opened = service.openPptx(path);
        const reader: PptxReader = new PptxReader(opened, () => this.readers.delete(reader));
        this.readers.add(reader);
        return reader;
    }

    /** @internal */
    assertActive(): void {
        if (this.closed) {
            throw new OpenPPTError('session/disposed', 'the OpenPPT session has been disposed; create a new instance');
        }
    }

    /** @internal */
    release(deck: Deck): void {
        this.decks.delete(deck);
    }

    private attach(
        opened: service.DeckHandle,
        format: PresentationFormat = 'pptx',
        intent: DeckIntent = 'edit',
        importLosses: ReadonlyArray<ImportLoss> = [],
    ): Deck {
        const deck = Deck.attach(this, opened, format, intent, importLosses);
        this.decks.add(deck);
        return deck;
    }
}

function createSize(size: DeckSize): string | CustomDeckSize {
    if (typeof size === 'string') {
        return size;
    }
    if (!isPositive(size.widthPt) || !isPositive(size.heightPt)) {
        throw new OpenPPTError(
            'session/argument-invalid',
            'custom slide size needs finite positive `widthPt` and `heightPt`',
        );
    }
    return { widthPt: size.widthPt, heightPt: size.heightPt };
}

function isPositive(value: number): boolean {
    return Number.isFinite(value) && value > 0;
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
