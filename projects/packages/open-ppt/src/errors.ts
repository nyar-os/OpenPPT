/**
 * Stable error surface of `@wae/open-ppt`.
 *
 * Codes are contract, not prose: callers compare them, localize around them and
 * never parse the message. A failure therefore keeps its code even when the
 * text is translated.
 */

/** Every stable code the authoring surface may report. */
export const errorCodes = [
    'session/disposed',
    'session/argument-invalid',
    'session/budget-exceeded',
    'deck/not-found',
    'deck/revision-conflict',
    'object/not-found',
    'object/type-mismatch',
    'object/unsupported',
    'format/unsupported',
    'format/invalid-package',
    'format/encrypted',
    'format/macro-refused',
    'asset/missing',
    'asset/in-use',
    'asset/too-large',
    'font/missing',
    'render/backend-unavailable',
    'export/loss-exceeded',
    'playback/invalid-state',
    'capability/not-implemented',
    'io/read',
    'io/write',
    'io/exists',
    'io/failure',
    'binding/failure',
] as const;

/** Union of {@link errorCodes}. */
export type OpenPPTErrorCode = (typeof errorCodes)[number];

/** Optional locators a failure can carry so callers can point at the object. */
export interface OpenPPTErrorDetails {
    readonly revision?: number;
    readonly slideId?: string;
    readonly elementId?: string;
    /** Presentation format involved in the failure, without the leading dot. */
    readonly format?: string;
    /** Authoring capability involved in the failure, e.g. `deck.export`. */
    readonly feature?: string;
}

/**
 * Kinds an `object/*` payload may name.
 *
 * The set is closed for authored objects; `presentation` names an imported
 * playback handle, and the binding refuses anything else.
 */
export type OpenPPTObjectKind = 'deck' | 'slide' | 'element' | 'layout' | 'master' | 'theme' | 'asset' | 'presentation';

/** A payload that carries no parameters of its own. */
type NoParams = { readonly [key: string]: never };

/**
 * Frozen payload of every code.
 *
 * These shapes are contract: the native binding validates against them and the
 * product renders copy from them, so a code never carries prose it has to be
 * re-parsed. Only the translation changes between locales, never the payload.
 */
export interface ErrorParamsByCode {
    'session/disposed': NoParams;
    'session/argument-invalid': { readonly reason: string };
    'session/budget-exceeded': NoParams;
    'deck/not-found': NoParams;
    'deck/revision-conflict': { readonly actual: number; readonly expected?: number };
    'object/not-found': { readonly kind: OpenPPTObjectKind; readonly id: string };
    'object/type-mismatch': { readonly kind: OpenPPTObjectKind; readonly id: string };
    'object/unsupported': { readonly feature: string };
    'format/unsupported': { readonly format: string };
    'format/invalid-package': { readonly reason: string };
    'format/encrypted': NoParams;
    'format/macro-refused': NoParams;
    'asset/missing': { readonly assetId: string };
    'asset/in-use': { readonly assetId: string };
    'asset/too-large': { readonly assetId: string; readonly bytes?: number };
    'font/missing': { readonly font: string };
    'render/backend-unavailable': NoParams;
    'export/loss-exceeded': { readonly count: number; readonly policy: string };
    'playback/invalid-state': { readonly reason: string };
    'capability/not-implemented': { readonly feature: string };
    'io/read': { readonly path: string; readonly kind: string };
    'io/write': { readonly path: string };
    'io/exists': { readonly path: string };
    'io/failure': { readonly kind: string };
    'binding/failure': { readonly reason: string; readonly code?: string };
}

/** Union of every frozen payload. */
export type OpenPPTErrorParams = ErrorParamsByCode[OpenPPTErrorCode];

/**
 * One failure as a discriminated union: narrowing on `code` narrows `params`.
 *
 * Callers branch on {@link OpenPPTFailure.code} and read the payload without a
 * cast, so no branch depends on the wording of a message.
 */
export type OpenPPTFailure = {
    [Code in OpenPPTErrorCode]: { readonly code: Code; readonly params: ErrorParamsByCode[Code] };
}[OpenPPTErrorCode];

/** Field kinds the validator understands; `?` marks an optional field. */
type FieldKind = 'string' | 'number' | 'string?' | 'number?';

const paramSpecs: Readonly<Record<OpenPPTErrorCode, Readonly<Record<string, FieldKind>>>> = {
    'session/disposed': {},
    'session/argument-invalid': { reason: 'string' },
    'session/budget-exceeded': {},
    'deck/not-found': {},
    'deck/revision-conflict': { actual: 'number', expected: 'number?' },
    'object/not-found': { kind: 'string', id: 'string' },
    'object/type-mismatch': { kind: 'string', id: 'string' },
    'object/unsupported': { feature: 'string' },
    'format/unsupported': { format: 'string' },
    'format/invalid-package': { reason: 'string' },
    'format/encrypted': {},
    'format/macro-refused': {},
    'asset/missing': { assetId: 'string' },
    'asset/in-use': { assetId: 'string' },
    'asset/too-large': { assetId: 'string', bytes: 'number?' },
    'font/missing': { font: 'string' },
    'render/backend-unavailable': {},
    'export/loss-exceeded': { count: 'number', policy: 'string' },
    'playback/invalid-state': { reason: 'string' },
    'capability/not-implemented': { feature: 'string' },
    'io/read': { path: 'string', kind: 'string' },
    'io/write': { path: 'string' },
    'io/exists': { path: 'string' },
    'io/failure': { kind: 'string' },
    'binding/failure': { reason: 'string', code: 'string?' },
};

/** Why a payload was refused. */
export interface ErrorParamsRejection {
    /** `unknown-code`: the code is outside the contract; `malformed`: the payload does not match its shape. */
    readonly kind: 'unknown-code' | 'malformed';
    /** Machine-readable reason, e.g. `missing key `id``. */
    readonly reason: string;
}

/** Outcome of validating one payload against its code's frozen shape. */
export type ErrorParamsResult =
    | { readonly ok: true; readonly params: OpenPPTErrorParams }
    | { readonly ok: false; readonly rejection: ErrorParamsRejection };

/**
 * Validates one payload against its code's frozen shape.
 *
 * A payload must be a plain object carrying exactly the declared keys, each of
 * the declared type. Unknown codes, prose, arrays, missing keys, extra keys and
 * mistyped values are refused so nothing half-formed reaches a branch.
 */
export function parseErrorParams(code: string, payload: unknown): ErrorParamsResult {
    if (!isContractedCode(code)) {
        return { ok: false, rejection: { kind: 'unknown-code', reason: 'unknown code ' + code } };
    }
    if (typeof payload !== 'object' || payload === null || Array.isArray(payload)) {
        return { ok: false, rejection: { kind: 'malformed', reason: `${code} needs an object payload` } };
    }
    const spec = paramSpecs[code];
    const record = payload as Record<string, unknown>;
    for (const key of Object.keys(record)) {
        if (!(key in spec)) {
            return { ok: false, rejection: { kind: 'malformed', reason: 'unknown key ' + key + ' for ' + code } };
        }
    }
    for (const [key, kind] of Object.entries(spec)) {
        const optional = kind.endsWith('?');
        const value = record[key];
        if (value === undefined || (optional && value === null)) {
            if (optional) {
                continue;
            }
            return { ok: false, rejection: { kind: 'malformed', reason: 'missing key ' + key + ' for ' + code } };
        }
        const expected = kind.startsWith('number') ? 'number' : 'string';
        if (typeof value !== expected || (expected === 'number' && !Number.isFinite(value as number))) {
            return { ok: false, rejection: { kind: 'malformed', reason: 'key ' + key + ' must be a ' + expected + ' for ' + code } };
        }
    }
    return { ok: true, params: record as OpenPPTErrorParams };
}

/** Whether a code belongs to the frozen contract. */
export function isContractedCode(code: string): code is OpenPPTErrorCode {
    return Object.prototype.hasOwnProperty.call(paramSpecs, code);
}

/**
 * Resolves one code and payload into a discriminated failure.
 *
 * Unknown codes and malformed payloads land on `binding/failure`, keeping the
 * original code and text as payload instead of guessing at parameters. That
 * fallback is itself structured, so every branch stays exhaustive.
 */
export function failureFrom(code: string, message: string, payload: unknown): OpenPPTFailure {
    const parsed = parseErrorParams(code, payload);
    if (parsed.ok) {
        return { code: code as OpenPPTErrorCode, params: parsed.params } as OpenPPTFailure;
    }
    return { code: 'binding/failure', params: { reason: message, code } };
}

/** Options accepted by {@link OpenPPTError}. */
export interface OpenPPTErrorOptions {
    readonly recoverable?: boolean;
    readonly details?: OpenPPTErrorDetails;
    readonly params?: unknown;
    readonly cause?: unknown;
}

/**
 * Payload the class derives when a caller omits one.
 *
 * Codes whose shape is only a `reason` take the message; codes whose shape is a
 * locator take it from `details`. Codes that address an object by `kind`/`id`
 * (or by `path`, `assetId`, `count`, ...) must supply `params` explicitly, and
 * an omission stays unstructured so the failure surfaces as `binding/failure`
 * instead of a half-filled payload.
 */
function defaultParams(code: string, message: string, details: OpenPPTErrorDetails): unknown {
    switch (code) {
        case 'session/argument-invalid':
        case 'format/invalid-package':
        case 'playback/invalid-state':
        case 'binding/failure':
            return { reason: message };
        case 'object/unsupported':
        case 'capability/not-implemented':
            return details.feature === undefined ? undefined : { feature: details.feature };
        case 'format/unsupported':
            return details.format === undefined ? undefined : { format: details.format };
        case 'deck/revision-conflict':
            return details.revision === undefined ? undefined : { actual: details.revision };
        case 'session/disposed':
        case 'session/budget-exceeded':
        case 'deck/not-found':
        case 'format/encrypted':
        case 'format/macro-refused':
        case 'render/backend-unavailable':
            return {};
        default:
            return undefined;
    }
}

/** Every failure thrown or rejected by this package. */
export class OpenPPTError extends Error {
    /** The code and its frozen payload, as a discriminated union. */
    readonly failure: OpenPPTFailure;
    /** Whether retrying with different input or state can succeed. */
    readonly recoverable: boolean;
    /** Optional locators (`revision`, `slideId`, `elementId`, `format`, `feature`). */
    readonly details: OpenPPTErrorDetails;

    constructor(code: string, message: string, options: OpenPPTErrorOptions = {}) {
        super(message, options.cause === undefined ? undefined : { cause: options.cause });
        this.name = 'OpenPPTError';
        const params = options.params ?? defaultParams(code, message, options.details ?? {});
        this.failure = failureFrom(code, message, params);
        this.recoverable = options.recoverable ?? false;
        this.details = Object.freeze({ ...options.details });
    }

    /** Stable, comparable code. */
    get code(): OpenPPTErrorCode {
        return this.failure.code;
    }

    /** Frozen parameters of {@link code}. */
    get params(): OpenPPTErrorParams {
        return this.failure.params;
    }
}

/**
 * Reported by declared methods whose backing capability is not wired yet.
 *
 * The scaffold uses one code for every unimplemented entry point instead of
 * inventing per-method placeholders; each method drops this rejection as it is
 * implemented, and the code disappears with the last one.
 */
export class NotImplementedError extends OpenPPTError {
    constructor(feature: string, message = `\`${feature}\` is not implemented yet`) {
        super('capability/not-implemented', message, { recoverable: false, details: { feature }, params: { feature } });
        this.name = 'NotImplementedError';
    }
}

/** Narrows an unknown thrown value to {@link OpenPPTError}. */
export function isOpenPPTError(value: unknown): value is OpenPPTError {
    return value instanceof OpenPPTError;
}

/**
 * Product copy for each stable code, rendered from its typed `params`.
 *
 * The binding's own text is only a fallback for codes this catalog does not
 * carry yet; the machine-readable `code` and `params` never depend on locale.
 */
const messageCatalog: Readonly<Record<string, Readonly<Record<string, string>>>> = {
    en: {
        'session/disposed': 'the OpenPPT session has been disposed; create a new instance',
        'session/argument-invalid': 'the request was rejected: {reason}',
        'session/budget-exceeded': 'the operation exceeded the session budget',
        'deck/not-found': 'the deck handle is not open',
        'deck/revision-conflict': 'the deck changed at revision {actual}; the edit expected {expected}',
        'object/not-found': 'the `{kind}` `{id}` was not found',
        'object/type-mismatch': 'the `{kind}` `{id}` does not accept this operation',
        'object/unsupported': 'the `{feature}` is not supported yet',
        'format/unsupported': '`{format}` is not an offered format',
        'format/invalid-package': 'the package failed validation: {reason}',
        'format/encrypted': 'encrypted packages cannot be opened',
        'format/macro-refused': 'macro-bearing packages are refused',
        'asset/missing': 'asset `{assetId}` is missing from the package',
        'asset/in-use': 'asset `{assetId}` is still placed by an element',
        'asset/too-large': 'the asset `{assetId}` exceeds the size budget',
        'font/missing': 'the required font `{font}` is not available',
        'render/backend-unavailable': 'no render backend is available on this platform',
        'export/loss-exceeded': 'the export dropped {count} constructs and the loss policy is `{policy}`',
        'playback/invalid-state': 'playback cannot run in the current state: {reason}',
        'capability/not-implemented': 'the `{feature}` capability is not implemented yet',
        'io/read': 'could not read `{path}` ({kind})',
        'io/write': 'could not publish `{path}`',
        'io/exists': '`{path}` already exists; pass overwrite to replace it',
        'io/failure': 'the file operation failed ({kind})',
        'binding/failure': 'the native binding reported a failure: {reason}',
    },
    'zh-CN': {
        'session/disposed': 'OpenPPT 会话已释放；请创建新实例',
        'session/argument-invalid': '请求被拒绝：{reason}',
        'session/budget-exceeded': '操作超出会话预算',
        'deck/not-found': '该幻灯片组句柄未打开',
        'deck/revision-conflict': '幻灯片组已变更到修订 {actual}；本次编辑期望 {expected}',
        'object/not-found': '未找到 `{kind}` `{id}`',
        'object/type-mismatch': '`{kind}` `{id}` 不接受此操作',
        'object/unsupported': '`{feature}` 尚未支持',
        'format/unsupported': '`{format}` 不是可提供的格式',
        'format/invalid-package': '包校验失败：{reason}',
        'format/encrypted': '无法打开加密包',
        'format/macro-refused': '拒绝包含宏的包',
        'asset/missing': '素材 `{assetId}` 在包中缺失',
        'asset/in-use': '素材 `{assetId}` 仍被元素引用',
        'asset/too-large': '素材 `{assetId}` 超出体积预算',
        'font/missing': '所需字体 `{font}` 不可用',
        'render/backend-unavailable': '当前平台没有可用的渲染后端',
        'export/loss-exceeded': '导出丢弃了 {count} 个构造，损失策略为 `{policy}`',
        'playback/invalid-state': '当前状态无法播放：{reason}',
        'capability/not-implemented': '`{feature}` 能力尚未实现',
        'io/read': '无法读取 `{path}`（{kind}）',
        'io/write': '无法发布 `{path}`',
        'io/exists': '`{path}` 已存在；传入 overwrite 以替换',
        'io/failure': '文件操作失败（{kind}）',
        'binding/failure': '原生绑定返回了失败：{reason}',
    },
};

let currentLocale = 'en';

/** Selects the locale used to render error copy; `zh*` maps to `zh-CN`. */
export function setErrorLocale(tag: string): void {
    currentLocale = tag.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';
}

/**
 * Renders the product copy for a code and its params.
 *
 * Unknown codes and missing translations fall back to the binding's own text,
 * so a replaced or removed native message never changes which branch a caller
 * takes: that branch is decided by `code` and `params`.
 */
export function renderErrorCopy(code: string, params: OpenPPTErrorParams, fallback: string): string {
    const template = messageCatalog[currentLocale]?.[code];
    if (template === undefined) {
        return fallback;
    }
    const payload = params as Readonly<Record<string, unknown>>;
    let complete = true;
    const rendered = template.replace(/\{(\w+)\}/g, (match, key: string) => {
        if (payload[key] === undefined) {
            complete = false;
            return match;
        }
        return String(payload[key]);
    });
    return complete ? rendered : fallback;
}
