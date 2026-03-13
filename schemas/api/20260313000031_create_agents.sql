-- Agents table.
--
-- An agent is a YAML-defined LLM orchestrator that calls user functions as
-- tools.  The definition is stored here after `flux agent deploy` parses and
-- validates the YAML file.
--
-- Columns
-- -------
--   name          – unique agent name (used in CLI and API paths)
--   model         – LLM model identifier (e.g. gpt-4o, gpt-4o-mini)
--   system        – system prompt text
--   tools         – ordered list of function names the agent may call
--   llm_url       – chat completions endpoint (OpenAI-compatible)
--   llm_secret    – name of the project secret holding the LLM API key
--   max_turns     – maximum tool-call rounds before aborting (default 25)
--   temperature   – sampling temperature (default 0.7)
--   config        – extra model params: top_p, max_tokens (JSONB)
--   input_schema  – JSON Schema for the input payload (optional)
--   output_schema – JSON Schema for the expected output (optional)
--   rules         – guard-rail rules: require, max_calls (JSONB array)
--   content_sha   – SHA-256 of the source YAML for version tracking
--   deployed_at   – timestamp of first deploy
--   updated_at    – timestamp of last deploy / update

CREATE TABLE IF NOT EXISTS flux.agents (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT        NOT NULL UNIQUE,
    model         TEXT        NOT NULL,
    system        TEXT        NOT NULL,
    tools         TEXT[]      NOT NULL DEFAULT '{}',
    llm_url       TEXT        NOT NULL DEFAULT 'https://api.openai.com/v1/chat/completions',
    llm_secret    TEXT        NOT NULL DEFAULT 'FLUXBASE_LLM_KEY',
    max_turns     INT         NOT NULL DEFAULT 25,
    temperature   REAL        NOT NULL DEFAULT 0.7,
    config        JSONB       NOT NULL DEFAULT '{}',
    input_schema  JSONB,
    output_schema JSONB,
    rules         JSONB       NOT NULL DEFAULT '[]',
    content_sha   TEXT        NOT NULL,
    deployed_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_agents_name ON flux.agents (name);
