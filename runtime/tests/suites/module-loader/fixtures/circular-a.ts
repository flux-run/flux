// Circular dependency — node A imports from B, B imports from A.
// Both export a lazy accessor so the circular reference is resolved at call time.
// This is the most common real-world circular pattern (e.g. type registries).

import { getB } from "./circular-b.js";

export function getA(): string { return "A"; }
export function callB(): string { return getB(); }
