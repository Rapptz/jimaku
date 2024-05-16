let wasm;

const cachedTextDecoder = (typeof TextDecoder !== 'undefined' ? new TextDecoder('utf-8', { ignoreBOM: true, fatal: true }) : { decode: () => { throw Error('TextDecoder not available') } } );

if (typeof TextDecoder !== 'undefined') { cachedTextDecoder.decode(); };

let cachedUint8Memory0 = null;

function getUint8Memory0() {
    if (cachedUint8Memory0 === null || cachedUint8Memory0.byteLength === 0) {
        cachedUint8Memory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8Memory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return cachedTextDecoder.decode(getUint8Memory0().subarray(ptr, ptr + len));
}

let cachedInt32Memory0 = null;

function getInt32Memory0() {
    if (cachedInt32Memory0 === null || cachedInt32Memory0.byteLength === 0) {
        cachedInt32Memory0 = new Int32Array(wasm.memory.buffer);
    }
    return cachedInt32Memory0;
}

let WASM_VECTOR_LEN = 0;

const cachedTextEncoder = (typeof TextEncoder !== 'undefined' ? new TextEncoder('utf-8') : { encode: () => { throw Error('TextEncoder not available') } } );

const encodeString = (typeof cachedTextEncoder.encodeInto === 'function'
    ? function (arg, view) {
    return cachedTextEncoder.encodeInto(arg, view);
}
    : function (arg, view) {
    const buf = cachedTextEncoder.encode(arg);
    view.set(buf);
    return {
        read: arg.length,
        written: buf.length
    };
});

function passStringToWasm0(arg, malloc, realloc) {

    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8Memory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8Memory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }

    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8Memory0().subarray(ptr + offset, ptr + len);
        const ret = encodeString(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

const heap = new Array(128).fill(undefined);

heap.push(undefined, null, true, false);

let heap_next = heap.length;

function addHeapObject(obj) {
    if (heap_next === heap.length) heap.push(heap.length + 1);
    const idx = heap_next;
    heap_next = heap[idx];

    heap[idx] = obj;
    return idx;
}

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
    return instance.ptr;
}

let cachedUint32Memory0 = null;

function getUint32Memory0() {
    if (cachedUint32Memory0 === null || cachedUint32Memory0.byteLength === 0) {
        cachedUint32Memory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32Memory0;
}

function getObject(idx) { return heap[idx]; }

function dropObject(idx) {
    if (idx < 132) return;
    heap[idx] = heap_next;
    heap_next = idx;
}

function takeObject(idx) {
    const ret = getObject(idx);
    dropObject(idx);
    return ret;
}

function getArrayJsValueFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    const mem = getUint32Memory0();
    const slice = mem.subarray(ptr / 4, ptr / 4 + len);
    const result = [];
    for (let i = 0; i < slice.length; i++) {
        result.push(takeObject(slice[i]));
    }
    return result;
}
/**
* @param {string} input
* @param {Options} options
* @returns {(Element)[]}
*/
export function parse(input, options) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(input, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        _assertClass(options, Options);
        var ptr1 = options.__destroy_into_raw();
        wasm.parse(retptr, ptr0, len0, ptr1);
        var r0 = getInt32Memory0()[retptr / 4 + 0];
        var r1 = getInt32Memory0()[retptr / 4 + 1];
        var v3 = getArrayJsValueFromWasm0(r0, r1).slice();
        wasm.__wbindgen_free(r0, r1 * 4, 4);
        return v3;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
* The kind of element that has been parsed
*/
export const ElementKind = Object.freeze({ AudioTerm:0,"0":"AudioTerm",DeviceCompatibility:1,"1":"DeviceCompatibility",Episode:2,"2":"Episode",EpisodeTitle:3,"3":"EpisodeTitle",EpisodeAlt:4,"4":"EpisodeAlt",FileChecksum:5,"5":"FileChecksum",FileExtension:6,"6":"FileExtension",Language:7,"7":"Language",Other:8,"8":"Other",ReleaseGroup:9,"9":"ReleaseGroup",ReleaseInformation:10,"10":"ReleaseInformation",ReleaseVersion:11,"11":"ReleaseVersion",Season:12,"12":"Season",Source:13,"13":"Source",Subtitles:14,"14":"Subtitles",Title:15,"15":"Title",Type:16,"16":"Type",VideoResolution:17,"17":"VideoResolution",VideoTerm:18,"18":"VideoTerm",Volume:19,"19":"Volume",Year:20,"20":"Year",Date:21,"21":"Date", });

const ElementFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_element_free(ptr >>> 0));
/**
*/
export class Element {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(Element.prototype);
        obj.__wbg_ptr = ptr;
        ElementFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        ElementFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_element_free(ptr);
    }
    /**
    * @returns {ElementKind}
    */
    get kind() {
        const ret = wasm.__wbg_get_element_kind(this.__wbg_ptr);
        return ret;
    }
    /**
    * @param {ElementKind} arg0
    */
    set kind(arg0) {
        wasm.__wbg_set_element_kind(this.__wbg_ptr, arg0);
    }
    /**
    * @returns {string}
    */
    get value() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.__wbg_get_element_value(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
    * @param {string} arg0
    */
    set value(arg0) {
        const ptr0 = passStringToWasm0(arg0, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.__wbg_set_element_value(this.__wbg_ptr, ptr0, len0);
    }
}

const OptionsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_options_free(ptr >>> 0));
/**
*/
export class Options {

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        OptionsFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_options_free(ptr);
    }
    /**
    * @returns {boolean}
    */
    get episode() {
        const ret = wasm.__wbg_get_options_episode(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
    * @param {boolean} arg0
    */
    set episode(arg0) {
        wasm.__wbg_set_options_episode(this.__wbg_ptr, arg0);
    }
    /**
    * @returns {boolean}
    */
    get episode_title() {
        const ret = wasm.__wbg_get_options_episode_title(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
    * @param {boolean} arg0
    */
    set episode_title(arg0) {
        wasm.__wbg_set_options_episode_title(this.__wbg_ptr, arg0);
    }
    /**
    * @returns {boolean}
    */
    get file_checksum() {
        const ret = wasm.__wbg_get_options_file_checksum(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
    * @param {boolean} arg0
    */
    set file_checksum(arg0) {
        wasm.__wbg_set_options_file_checksum(this.__wbg_ptr, arg0);
    }
    /**
    * @returns {boolean}
    */
    get file_extension() {
        const ret = wasm.__wbg_get_options_file_extension(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
    * @param {boolean} arg0
    */
    set file_extension(arg0) {
        wasm.__wbg_set_options_file_extension(this.__wbg_ptr, arg0);
    }
    /**
    * @returns {boolean}
    */
    get release_group() {
        const ret = wasm.__wbg_get_options_release_group(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
    * @param {boolean} arg0
    */
    set release_group(arg0) {
        wasm.__wbg_set_options_release_group(this.__wbg_ptr, arg0);
    }
    /**
    * @returns {boolean}
    */
    get season() {
        const ret = wasm.__wbg_get_options_season(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
    * @param {boolean} arg0
    */
    set season(arg0) {
        wasm.__wbg_set_options_season(this.__wbg_ptr, arg0);
    }
    /**
    * @returns {boolean}
    */
    get title() {
        const ret = wasm.__wbg_get_options_title(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
    * @param {boolean} arg0
    */
    set title(arg0) {
        wasm.__wbg_set_options_title(this.__wbg_ptr, arg0);
    }
    /**
    * @returns {boolean}
    */
    get video_resolution() {
        const ret = wasm.__wbg_get_options_video_resolution(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
    * @param {boolean} arg0
    */
    set video_resolution(arg0) {
        wasm.__wbg_set_options_video_resolution(this.__wbg_ptr, arg0);
    }
    /**
    * @returns {boolean}
    */
    get year() {
        const ret = wasm.__wbg_get_options_year(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
    * @param {boolean} arg0
    */
    set year(arg0) {
        wasm.__wbg_set_options_year(this.__wbg_ptr, arg0);
    }
    /**
    * @returns {boolean}
    */
    get date() {
        const ret = wasm.__wbg_get_options_date(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
    * @param {boolean} arg0
    */
    set date(arg0) {
        wasm.__wbg_set_options_date(this.__wbg_ptr, arg0);
    }
    /**
    */
    constructor() {
        const ret = wasm.options_new();
        this.__wbg_ptr = ret >>> 0;
        return this;
    }
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);

            } catch (e) {
                if (module.headers.get('Content-Type') != 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else {
                    throw e;
                }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);

    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };

        } else {
            return instance;
        }
    }
}

function __wbg_get_imports() {
    const imports = {};
    imports.wbg = {};
    imports.wbg.__wbg_element_new = function(arg0) {
        const ret = Element.__wrap(arg0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_throw = function(arg0, arg1) {
        throw new Error(getStringFromWasm0(arg0, arg1));
    };

    return imports;
}

function __wbg_init_memory(imports, maybe_memory) {

}

function __wbg_finalize_init(instance, module) {
    wasm = instance.exports;
    __wbg_init.__wbindgen_wasm_module = module;
    cachedInt32Memory0 = null;
    cachedUint32Memory0 = null;
    cachedUint8Memory0 = null;


    return wasm;
}

function initSync(module) {
    if (wasm !== undefined) return wasm;

    const imports = __wbg_get_imports();

    __wbg_init_memory(imports);

    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }

    const instance = new WebAssembly.Instance(module, imports);

    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(input) {
    if (wasm !== undefined) return wasm;

    if (typeof input === 'undefined') {
        input = new URL('anitomy_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof input === 'string' || (typeof Request === 'function' && input instanceof Request) || (typeof URL === 'function' && input instanceof URL)) {
        input = fetch(input);
    }

    __wbg_init_memory(imports);

    const { instance, module } = await __wbg_load(await input, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync }
export default __wbg_init;
