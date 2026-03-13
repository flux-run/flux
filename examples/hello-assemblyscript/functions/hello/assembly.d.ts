// AssemblyScript built-in declarations for editor/tsc support.
// The real AS compiler (asc) has these built-in; this file enables IntelliSense.
// Do not edit — regenerated on `flux function create`.

declare type i8   = number;
declare type i16  = number;
declare type i32  = number;
declare type i64  = number;
declare type isize = number;
declare type u8   = number;
declare type u16  = number;
declare type u32  = number;
declare type u64  = number;
declare type usize = number;
declare type f32  = number;
declare type f64  = number;
declare type bool = boolean;
declare type v128 = never;

/** Reinterpret the bits of a value as a different type (AssemblyScript built-in). */
declare function changetype<T>(value: unknown): T;
declare function unreachable(): never;

// Augment the global String constructor to add AssemblyScript's UTF8 helpers.
interface StringConstructor {
  UTF8: {
    encode(str: string, nullTerminated?: bool): ArrayBuffer;
    decode(buf: ArrayBuffer | Uint8Array, nullTerminated?: bool): string;
    byteLength(str: string, nullTerminated?: bool): i32;
  };
}
