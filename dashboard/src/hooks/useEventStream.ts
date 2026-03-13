/**
 * useEventStream — subscribe to a Flux SSE stream endpoint.
 *
 * Automatically handles:
 *  - Auth header (JWT passed as ?token= query param since EventSource doesn't
 *    support custom headers in all browsers)
 *  - Cursor resumption via Last-Event-ID (EventSource does this natively)
 *  - Reconnection with exponential back-off (native EventSource behaviour)
 *  - Cleanup on unmount
 *
 * Usage:
 *   const { events, connected } = useEventStream<ExecutionEvent>('executions');
 *   const { events, connected } = useEventStream<AppEvent>('events');
 *   const { events, connected } = useEventStream<MutationEvent>('mutations');
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import { getToken } from '@/lib/auth';

export type StreamName = 'events' | 'executions' | 'mutations';

export interface StreamOptions {
  /** Max events to keep in memory (default: 200) */
  maxEvents?: number;
  /** ISO 8601 start cursor — defaults to "now" (tail mode) */
  since?: string;
  /** Pause the stream without unmounting */
  paused?: boolean;
}

export interface UseEventStreamResult<T> {
  events: T[];
  connected: boolean;
  error: string | null;
  clear: () => void;
}

const BASE_URL = '/flux/api';

export function useEventStream<T = Record<string, unknown>>(
  stream: StreamName,
  options: StreamOptions = {},
): UseEventStreamResult<T> {
  const { maxEvents = 200, since, paused = false } = options;
  const { token } = { token: getToken() };

  const [events, setEvents] = useState<T[]>([]);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const esRef = useRef<EventSource | null>(null);

  const clear = useCallback(() => setEvents([]), []);

  useEffect(() => {
    if (paused || !token) return;

    const params = new URLSearchParams();
    // Pass JWT as query param — EventSource can't set Authorization header.
    params.set('token', token);
    if (since) params.set('since', since);

    const url = `${BASE_URL}/stream/${stream}?${params.toString()}`;
    const es = new EventSource(url);
    esRef.current = es;

    es.onopen = () => {
      setConnected(true);
      setError(null);
    };

    // Each stream uses a named event type matching the endpoint name.
    const eventType = stream === 'executions' ? 'execution'
      : stream === 'mutations' ? 'mutation'
      : 'event';

    es.addEventListener(eventType, (e: MessageEvent) => {
      try {
        const data = JSON.parse(e.data) as T;
        setEvents(prev => {
          const next = [...prev, data];
          return next.length > maxEvents ? next.slice(next.length - maxEvents) : next;
        });
      } catch {
        // malformed JSON — ignore
      }
    });

    es.onerror = () => {
      setConnected(false);
      setError('Stream disconnected — reconnecting…');
      // EventSource reconnects automatically; we just update state.
    };

    return () => {
      es.close();
      esRef.current = null;
      setConnected(false);
    };
  }, [stream, since, paused, maxEvents]);

  return { events, connected, error, clear };
}

// ── Typed helpers ─────────────────────────────────────────────────────────────

export interface AppEvent {
  id: string;
  event_type: string;
  table: string;
  operation: 'insert' | 'update' | 'delete';
  record_id: string | null;
  ts: string;
}

export interface ExecutionEvent {
  request_id: string;
  method: string;
  path: string;
  status: number | null;
  duration_ms: number | null;
  ok: boolean;
  ts: string;
}

export interface MutationEvent {
  request_id: string | null;
  table: string;
  operation: 'insert' | 'update' | 'delete';
  record_id: string | null;
  seq: number | null;
  ts: string;
}
