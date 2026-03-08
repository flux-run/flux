export default async function (ctx) {
  const { a = 0, b = 0, operation = "add" } = ctx.payload || {};
  const ops = {
    add: a + b,
    sub: a - b,
    mul: a * b,
    div: b !== 0 ? a / b : "division_by_zero",
  };
  const result = ops[operation];
  return {
    result: result !== undefined ? result : "unknown_operation",
    operation,
    a,
    b,
  };
}
