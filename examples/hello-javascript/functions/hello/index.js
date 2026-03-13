export default {
  __fluxbase: true,

  /** @param {{payload: any, ctx: import("./.flux/ctx.js").FluxCtx}} args */
  async execute({ payload, ctx }) {
    ctx.log("Running hello");

    return { ok: true };
  },
};
