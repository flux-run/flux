// Module-level side effect + singleton — tests that the module cache ensures
// this initializer runs exactly once regardless of how many times it is imported.
let _initCount = 0;
_initCount++; // runs at module evaluation time

export function getInitCount(): number { return _initCount; }
export const SINGLETON_ID = Math.floor(Math.random() * 0xFFFFFF);
