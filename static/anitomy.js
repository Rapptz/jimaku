/* This file is licensed under MPL-2.0 */
import init, { parse as parse_wasm, Options } from "./anitomy_bg.js";

const elementKindLookup = Object.freeze({
  0: "audio_term",
  1: "device_compatibility",
  2: "episode",
  3: "episode_title",
  4: "episode_alt",
  5: "file_checksum",
  6: "file_extension",
  7: "language",
  8: "other",
  9: "release_group",
  10: "release_information",
  11: "release_version",
  12: "season",
  13: "source",
  14: "subtitles",
  15: "title",
  16: "type",
  17: "video_resolution",
  18: "video_term",
  19: "volume",
  20: "year",
  21: "date",
});

function parse(input, opts) {
  let options = new Options();
  for(const [key, value] of Object.entries(opts ?? {})) {
    const descriptor = Object.getOwnPropertyDescriptor(Options.prototype, key);
    if(descriptor == null || descriptor.set == null) {
      throw new Error(`Unknown option: ${key}`);
    }
    descriptor.set.call(options, value);
  }
  let result = {};
  for(const element of parse_wasm(input, options)) {
    const key = elementKindLookup[element.kind];
    if(result.hasOwnProperty(key)) {
      if(Array.isArray(result[key])) {
        result[key].push(element.value);
      } else {
        result[key] = [result[key], element.value];
      }
    } else {
      result[key] = element.value;
    }
  }
  return result;
}

export { init, parse };
