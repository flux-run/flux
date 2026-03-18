export const Pool = Flux.postgres.NodePgPool;
export const types = Flux.postgres.nodePgTypes;

const pg = {
  Pool,
  types,
};

export default pg;