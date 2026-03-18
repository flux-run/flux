declare module "pg" {
  export type QueryResultRow = Record<string, unknown>;

  export interface QueryField {
    name: string;
    dataTypeID: number;
    format: "text" | "binary";
  }

  export interface QueryResult<Row extends QueryResultRow = QueryResultRow> {
    command: string | null;
    rowCount: number;
    rows: Row[];
    fields: QueryField[];
  }

  export interface QueryConfig {
    text: string;
    values?: unknown[];
    rowMode?: "array";
  }

  export interface PoolConfig {
    connectionString?: string;
  }

  export class Client {
    constructor(config?: PoolConfig);
    connect(): Promise<Client>;
    query<Row extends QueryResultRow = QueryResultRow>(queryText: string, values?: unknown[]): Promise<QueryResult<Row>>;
    query<Row extends QueryResultRow = QueryResultRow>(queryConfig: QueryConfig, values?: unknown[]): Promise<QueryResult<Row>>;
    release(): Promise<void>;
    end(): Promise<void>;
  }

  export class Pool {
    constructor(config?: PoolConfig);
    query<Row extends QueryResultRow = QueryResultRow>(queryText: string, values?: unknown[]): Promise<QueryResult<Row>>;
    query<Row extends QueryResultRow = QueryResultRow>(queryConfig: QueryConfig, values?: unknown[]): Promise<QueryResult<Row>>;
    connect(): Promise<Client>;
    end(): Promise<void>;
  }

  export const types: {
    builtins: Record<string, number>;
    getTypeParser(oid: number, format?: "text" | "binary"): (value: unknown) => unknown;
    setTypeParser(oid: number, parser: (value: unknown) => unknown): void;
  };

  const pg: {
    Pool: typeof Pool;
    Client: typeof Client;
    types: typeof types;
  };

  export default pg;
}