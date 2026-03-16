// Circular dependency — node B imports from A.
import { getA } from "./circular-a.js";

export function getB(): string { return "B"; }
export function callA(): string { return getA(); }
