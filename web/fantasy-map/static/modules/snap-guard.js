/**
 * The pure half of corridor matching: which in-flight match may still be
 * applied to a segment. Data in, data out — no fetch, no editor state,
 * so the Node tests can drive two overlapping requests through it.
 *
 * Segment identity already survives index shifts, but not supersession:
 * a slow road match resolving after a newer corridor match for the same
 * segment would otherwise overwrite the newer answer. Each request takes
 * a generation token for its segment, and only the newest one — still
 * spanning the endpoints it asked about — is allowed to write.
 */

const generations = new WeakMap(); // segment -> generation of its newest request

/** Claim the newest match for `segment` between `start` and `end`. */
export function beginSnap(segment, start, end) {
  const generation = (generations.get(segment) || 0) + 1;
  generations.set(segment, generation);
  return { segment, generation, start: [...start], end: [...end] };
}

/**
 * Whether `token`'s result may still be applied: no later request has
 * claimed the segment, and it still runs between the same endpoints.
 */
export function snapIsCurrent(token, start, end) {
  return (
    generations.get(token.segment) === token.generation &&
    samePoint(token.start, start) &&
    samePoint(token.end, end)
  );
}

const samePoint = (a, b) => Boolean(a && b && a[0] === b[0] && a[1] === b[1]);
