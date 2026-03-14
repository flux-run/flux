<?php
/**
 * Flux function handler — PHP 8.3 / Emscripten WASM
 *
 * The $ctx object provides access to Flux platform services:
 *   $ctx->db->query($sql, $params)   — data-engine query
 *   $ctx->secrets->get($key)         — secret store lookup
 *   $ctx->log($message, $level)      — structured log emission
 *   $ctx->queue->push($fn, $payload) — enqueue background job
 */

function handler(object $ctx, mixed $payload): mixed
{
    $name = $payload->name ?? 'world';

    $ctx->log("Handling request for: {$name}");

    return [
        'message' => "Hello, {$name}!",
        'runtime' => 'php-wasm',
    ];
}
