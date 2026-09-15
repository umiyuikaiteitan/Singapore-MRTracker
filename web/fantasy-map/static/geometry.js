/**
 * Client geometry for route construction.
 *
 * Coordinates are `[lat, lng]` pairs. Computation happens in a local
 * metre plane (equirectangular), accurate at city scale.
 *
 * Key entry point: `OFMGeometry.buildRouteGeometry(nodes, segments, radius)`
 * assembles the displayed polyline from manual anchor nodes and per-segment
 * guides, rounding road guides and cutting inward fillets at manual nodes.
 */
(function () {
  "use strict";

  const EARTH_RADIUS_M = 6371000;
  const STRAIGHT_TOLERANCE = (0.75 * Math.PI) / 180;
  const MAX_TANGENT_FRACTION = 0.48;
  const ARC_SAMPLES = 24;
  // findRadiusIssues reads a tight circle across very short legs as
  // corridor jitter, not a curve (see its doc comment). Both thresholds
  // are fixed properties of the geometry — neither may depend on the
  // minimum being checked, or a larger minimum could skip more vertices
  // and report fewer issues. Fillet arcs are sampled at legs of ~20 m
  // (`arcPoints`), so no honest arc has sub-8 m legs; and an arc's
  // measured radius is many times its leg, where jitter's is of the same
  // order.
  const JITTER_LEG_M = 8;
  const JITTER_RADIUS_RATIO = 3;

  const clamp = (value, lo, hi) => Math.max(lo, Math.min(hi, value));

  function haversineMeters(a, b) {
    const phi1 = (a[0] * Math.PI) / 180;
    const phi2 = (b[0] * Math.PI) / 180;
    const dPhi = ((b[0] - a[0]) * Math.PI) / 180;
    const dLambda = ((b[1] - a[1]) * Math.PI) / 180;
    const h =
      Math.sin(dPhi / 2) ** 2 +
      Math.cos(phi1) * Math.cos(phi2) * Math.sin(dLambda / 2) ** 2;
    return EARTH_RADIUS_M * 2 * Math.atan2(Math.sqrt(h), Math.sqrt(1 - h));
  }

  function routeLengthMeters(points) {
    let total = 0;
    for (let i = 1; i < points.length; i += 1) {
      total += haversineMeters(points[i - 1], points[i]);
    }
    return total;
  }

  /** Local metre plane centred on the given coordinates. */
  function createProjection(points) {
    const referenceLatitude =
      (points.reduce((sum, p) => sum + p[0], 0) / Math.max(1, points.length)) *
      (Math.PI / 180);
    const scale = Math.max(0.01, Math.cos(referenceLatitude));
    return {
      project: ([lat, lng]) => ({
        x: EARTH_RADIUS_M * ((lng * Math.PI) / 180) * scale,
        y: EARTH_RADIUS_M * ((lat * Math.PI) / 180),
      }),
      unproject: ({ x, y }) => [
        (y / EARTH_RADIUS_M) * (180 / Math.PI),
        (x / (EARTH_RADIUS_M * scale)) * (180 / Math.PI),
      ],
    };
  }

  const distXY = (a, b) => Math.hypot(b.x - a.x, b.y - a.y);

  function dedupeXY(points) {
    return points.filter(
      (point, index) => index === 0 || distXY(points[index - 1], point) > 0.05,
    );
  }

  /**
   * Inward tangent arc rounding `corner`; null when straight/degenerate.
   * A near-U-turn returns `tangentDistance: Infinity` so callers drop it.
   */
  function filletAt(previous, corner, next, radius) {
    const previousLength = distXY(previous, corner);
    const nextLength = distXY(corner, next);
    if (previousLength < 0.1 || nextLength < 0.1) return null;
    const towardPrevious = {
      x: (previous.x - corner.x) / previousLength,
      y: (previous.y - corner.y) / previousLength,
    };
    const towardNext = {
      x: (next.x - corner.x) / nextLength,
      y: (next.y - corner.y) / nextLength,
    };
    const interiorAngle = Math.acos(
      clamp(
        towardPrevious.x * towardNext.x + towardPrevious.y * towardNext.y,
        -1,
        1,
      ),
    );
    const deflection = Math.PI - interiorAngle;
    if (deflection < STRAIGHT_TOLERANCE) return null;
    if (interiorAngle < STRAIGHT_TOLERANCE) {
      return {
        center: corner,
        tangentStart: corner,
        tangentEnd: corner,
        startAngle: 0,
        sweep: Math.PI,
        tangentDistance: Number.POSITIVE_INFINITY,
        interiorAngle,
        previousLength,
        nextLength,
      };
    }
    const tangentDistance = radius / Math.tan(interiorAngle / 2);
    const bisector = {
      x: towardPrevious.x + towardNext.x,
      y: towardPrevious.y + towardNext.y,
    };
    const bisectorLength = Math.hypot(bisector.x, bisector.y);
    if (!Number.isFinite(tangentDistance) || bisectorLength < 1e-9) return null;
    const centerDistance = radius / Math.sin(interiorAngle / 2);
    const center = {
      x: corner.x + (bisector.x / bisectorLength) * centerDistance,
      y: corner.y + (bisector.y / bisectorLength) * centerDistance,
    };
    const tangentStart = {
      x: corner.x + towardPrevious.x * tangentDistance,
      y: corner.y + towardPrevious.y * tangentDistance,
    };
    const tangentEnd = {
      x: corner.x + towardNext.x * tangentDistance,
      y: corner.y + towardNext.y * tangentDistance,
    };
    const turnCross =
      -towardPrevious.x * towardNext.y + towardPrevious.y * towardNext.x;
    return {
      center,
      tangentStart,
      tangentEnd,
      startAngle: Math.atan2(
        tangentStart.y - center.y,
        tangentStart.x - center.x,
      ),
      sweep: Math.sign(turnCross || 1) * deflection,
      tangentDistance,
      interiorAngle,
      previousLength,
      nextLength,
    };
  }

  function arcPoints(fillet, radius) {
    const angleStep = Math.min((8 * Math.PI) / 180, 20 / radius);
    const steps = clamp(Math.ceil(Math.abs(fillet.sweep) / angleStep), 2, 96);
    const points = [];
    for (let step = 1; step <= steps; step += 1) {
      const angle = fillet.startAngle + fillet.sweep * (step / steps);
      points.push({
        x: fillet.center.x + radius * Math.cos(angle),
        y: fillet.center.y + radius * Math.sin(angle),
      });
    }
    return points;
  }

  /**
   * Round every corner of a road guide with fillets at `radius`, removing
   * corners whose fillet cannot fit (tightest first).
   */
  function roundGuideXY(points, radius) {
    const controls = dedupeXY(points);
    while (controls.length > 2) {
      let worstIndex = -1;
      let worstRatio = 1;
      for (let i = 1; i < controls.length - 1; i += 1) {
        const fillet = filletAt(
          controls[i - 1],
          controls[i],
          controls[i + 1],
          radius,
        );
        if (!fillet) continue;
        const available =
          Math.min(fillet.previousLength, fillet.nextLength) *
          MAX_TANGENT_FRACTION;
        const ratio = fillet.tangentDistance / Math.max(0.01, available);
        if (ratio > worstRatio) {
          worstRatio = ratio;
          worstIndex = i;
        }
      }
      if (worstIndex < 0) break;
      controls.splice(worstIndex, 1);
    }
    const output = [controls[0]];
    for (let i = 1; i < controls.length - 1; i += 1) {
      const fillet = filletAt(controls[i - 1], controls[i], controls[i + 1], radius);
      if (!fillet) {
        output.push(controls[i]);
        continue;
      }
      output.push(fillet.tangentStart, ...arcPoints(fillet, radius));
    }
    output.push(controls[controls.length - 1]);
    return dedupeXY(output);
  }

  /**
   * Cut an inward fillet at the junction between two assembled segment
   * point arrays. The corner (a manual node) is replaced by an arc that
   * fits the available leg lengths; the radius is reduced when necessary
   * so the fillet never consumes the neighbouring geometry.
   * Mutates `left` (trimmed tail) and returns the replacement points that
   * should precede `right.slice(1)`.
   */
  function junctionFillet(left, right, radius) {
    if (left.length < 2 || right.length < 2) return null;
    const corner = left[left.length - 1];
    const previous = left[left.length - 2];
    const next = right[1];
    let fillet = filletAt(previous, corner, next, radius);
    if (!fillet || !Number.isFinite(fillet.tangentDistance)) return null;
    const available =
      Math.min(distXY(previous, corner), distXY(corner, next)) *
      MAX_TANGENT_FRACTION;
    let effectiveRadius = radius;
    if (fillet.tangentDistance > available) {
      // Shrink the arc so it stays within both legs.
      effectiveRadius = available * Math.tan(fillet.interiorAngle / 2);
      if (effectiveRadius < 0.5) return null;
      fillet = filletAt(previous, corner, next, effectiveRadius);
      if (!fillet || !Number.isFinite(fillet.tangentDistance)) return null;
    }
    return {
      tangentStart: fillet.tangentStart,
      arc: arcPoints(fillet, effectiveRadius),
    };
  }

  /**
   * Build displayed geometry from manual nodes and per-segment guides.
   *
   * - `manual` segments are straight lines between their nodes.
   * - `road` segments use their guide, rounded to the minimum radius.
   * - `rail` segments use their guide exactly.
   * - Guides are re-anchored to the current node positions, so matching a
   *   corridor never moves a manually placed node.
   * - Interior manual nodes get an inward junction fillet at the radius.
   *
   * Returns `{points, segmentPoints}` where `segmentPoints[i]` is segment
   * i's share of the displayed polyline (junction arcs split halfway).
   *
   * `options.straight` (cableways) forces straight spans between nodes:
   * guides are ignored and no fillets are cut.
   */
  function buildRouteGeometry(nodes, segments, minimumRadiusMeters, options) {
    if (nodes.length < 2) {
      return { points: nodes.slice(), segmentPoints: [] };
    }
    const straight = Boolean(options && options.straight);
    const radius = Math.max(1, minimumRadiusMeters);
    const projection = createProjection(nodes);
    const segmentXY = [];
    for (let i = 0; i < nodes.length - 1; i += 1) {
      const segment =
        (!straight && segments[i]) || { profile: "manual", guide: [] };
      const guide = anchoredGuide(segment.guide, nodes[i], nodes[i + 1]);
      let pts = dedupeXY(guide.map(projection.project));
      if (pts.length < 2) {
        pts = [projection.project(nodes[i]), projection.project(nodes[i + 1])];
      }
      if (segment.profile === "road" && pts.length > 2) {
        pts = roundGuideXY(pts, radius);
      }
      segmentXY.push(pts);
    }

    // Fillet the corner at each interior manual node.
    for (let i = 0; !straight && i < segmentXY.length - 1; i += 1) {
      const left = segmentXY[i];
      const right = segmentXY[i + 1];
      const fillet = junctionFillet(left, right, radius);
      if (!fillet) continue;
      const half = Math.ceil(fillet.arc.length / 2);
      left.splice(
        left.length - 1,
        1,
        fillet.tangentStart,
        ...fillet.arc.slice(0, half),
      );
      right.splice(0, 1, ...fillet.arc.slice(half - 1));
    }

    const segmentPoints = segmentXY.map((pts) =>
      dedupeXY(pts).map(projection.unproject),
    );
    const points = [];
    segmentPoints.forEach((pts, index) => {
      points.push(...(index === 0 ? pts : pts.slice(1)));
    });
    return { points, segmentPoints };
  }

  /** Re-anchor a stored guide between the current segment endpoints. */
  function anchoredGuide(guide, start, end) {
    if (!guide || guide.length < 2) return [start, end];
    return [start, ...guide.slice(1, -1), end];
  }

  /**
   * Circle through three projected points, sampled between the outer two.
   * Null when they are collinear enough that the centre is unstable.
   */
  function circumscribedArc(a, b, c) {
    const denominator =
      2 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if (Math.abs(denominator) < 0.01) return null;
    const aSquared = a.x ** 2 + a.y ** 2;
    const bSquared = b.x ** 2 + b.y ** 2;
    const cSquared = c.x ** 2 + c.y ** 2;
    const center = {
      x:
        (aSquared * (b.y - c.y) +
          bSquared * (c.y - a.y) +
          cSquared * (a.y - b.y)) /
        denominator,
      y:
        (aSquared * (c.x - b.x) +
          bSquared * (a.x - c.x) +
          cSquared * (b.x - a.x)) /
        denominator,
    };
    const radius = Math.hypot(a.x - center.x, a.y - center.y);
    const turn = Math.PI * 2;
    const normalize = (angle) => ((angle % turn) + turn) % turn;
    const startAngle = Math.atan2(a.y - center.y, a.x - center.x);
    const middle = normalize(
      Math.atan2(b.y - center.y, b.x - center.x) - startAngle,
    );
    const end = normalize(
      Math.atan2(c.y - center.y, c.x - center.x) - startAngle,
    );
    // Sweep the way round that passes through the middle point.
    const sweep = middle <= end ? end : -(turn - end);
    const arc = [];
    for (let step = 0; step <= ARC_SAMPLES; step += 1) {
      const angle = startAngle + sweep * (step / ARC_SAMPLES);
      arc.push({
        x: center.x + radius * Math.cos(angle),
        y: center.y + radius * Math.sin(angle),
      });
    }
    return { radius, arc };
  }

  /**
   * Interior vertices of a displayed polyline whose curve is tighter than
   * the minimum radius, as
   * `[{index, coordinate, radiusMeters, arc}]` — `arc` being the offending
   * circle sampled between the vertex's two neighbours.
   *
   * The tolerance absorbs the sampling error of an arc built at exactly
   * the minimum, so honest geometry does not report itself. Two very
   * short legs meeting sharply are read as jitter in matched corridor
   * data rather than as a curve: an arc sampled that finely turns only a
   * few degrees from one point to the next, so a tight circle across
   * legs of its own scale is noise. That test uses the measured radius
   * and not the minimum, which keeps the count monotonic — raising a
   * line's minimum can never find fewer issues.
   */
  function findRadiusIssues(points, minimumRadiusMeters) {
    if (!points || points.length < 3) return [];
    const minimumRadius = Math.max(1, minimumRadiusMeters);
    const tolerance = Math.max(0.75, minimumRadius * 0.005);
    const projection = createProjection(points);
    const pts = points.map(projection.project);
    const issues = [];
    for (let i = 1; i < pts.length - 1; i += 1) {
      const circle = circumscribedArc(pts[i - 1], pts[i], pts[i + 1]);
      if (!circle || circle.radius + tolerance >= minimumRadius) continue;
      const leg = Math.min(distXY(pts[i - 1], pts[i]), distXY(pts[i], pts[i + 1]));
      if (leg < JITTER_LEG_M && circle.radius < leg * JITTER_RADIUS_RATIO) {
        continue;
      }
      issues.push({
        index: i,
        coordinate: points[i],
        radiusMeters: circle.radius,
        arc: circle.arc.map(projection.unproject),
      });
    }
    return issues;
  }

  /**
   * Closest point on a polyline. Returns
   * `{coordinate, distanceMeters, t}` with `t` the arc-length fraction.
   */
  function projectToPolyline(target, points) {
    if (points.length < 2) return null;
    const projection = createProjection([target]);
    const targetXY = projection.project(target);
    const pts = points.map(projection.project);
    const cumulative = [0];
    for (let i = 1; i < pts.length; i += 1) {
      cumulative.push(cumulative[i - 1] + distXY(pts[i - 1], pts[i]));
    }
    const total = cumulative[cumulative.length - 1] || 1;
    let best = null;
    for (let i = 1; i < pts.length; i += 1) {
      const a = pts[i - 1];
      const b = pts[i];
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const denominator = dx * dx + dy * dy;
      const amount =
        denominator === 0
          ? 0
          : clamp(
              ((targetXY.x - a.x) * dx + (targetXY.y - a.y) * dy) / denominator,
              0,
              1,
            );
      const candidate = { x: a.x + dx * amount, y: a.y + dy * amount };
      const distance = distXY(targetXY, candidate);
      if (!best || distance < best.distanceMeters) {
        best = {
          coordinate: projection.unproject(candidate),
          distanceMeters: distance,
          t: (cumulative[i - 1] + distXY(a, candidate)) / total,
        };
      }
    }
    return best;
  }

  /**
   * Split a polyline at the projection of `target` onto it. Returns
   * `{coordinate, before, after}` where both halves include the split
   * point, or null for degenerate input.
   */
  function splitAtProjection(points, target) {
    if (points.length < 2) return null;
    const projection = createProjection([target]);
    const targetXY = projection.project(target);
    const pts = points.map(projection.project);
    let best = null;
    for (let i = 1; i < pts.length; i += 1) {
      const a = pts[i - 1];
      const b = pts[i];
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const denominator = dx * dx + dy * dy;
      const amount =
        denominator === 0
          ? 0
          : clamp(
              ((targetXY.x - a.x) * dx + (targetXY.y - a.y) * dy) / denominator,
              0,
              1,
            );
      const candidate = { x: a.x + dx * amount, y: a.y + dy * amount };
      const distance = distXY(targetXY, candidate);
      if (!best || distance < best.distance) {
        best = { distance, index: i - 1, xy: candidate };
      }
    }
    if (!best) return null;
    const coordinate = projection.unproject(best.xy);
    return {
      coordinate,
      before: [...points.slice(0, best.index + 1), coordinate],
      after: [coordinate, ...points.slice(best.index + 1)],
    };
  }

  /** Coordinate at arc-length fraction `t` (0..1) along a polyline. */
  function pointAtFraction(points, t) {
    if (!points.length) return null;
    if (points.length === 1) return points[0];
    const projection = createProjection(points);
    const pts = points.map(projection.project);
    const cumulative = [0];
    for (let i = 1; i < pts.length; i += 1) {
      cumulative.push(cumulative[i - 1] + distXY(pts[i - 1], pts[i]));
    }
    const total = cumulative[cumulative.length - 1];
    const target = clamp(t, 0, 1) * total;
    for (let i = 1; i < pts.length; i += 1) {
      if (cumulative[i] >= target) {
        const span = cumulative[i] - cumulative[i - 1];
        const amount = span === 0 ? 0 : (target - cumulative[i - 1]) / span;
        return projection.unproject({
          x: pts[i - 1].x + (pts[i].x - pts[i - 1].x) * amount,
          y: pts[i - 1].y + (pts[i].y - pts[i - 1].y) * amount,
        });
      }
    }
    return points[points.length - 1];
  }

  /** Convert metre offsets `{north, east}` around an anchor to [lat, lng]. */
  function offsetToCoordinate(anchor, offset) {
    const scale = Math.max(0.01, Math.cos((anchor[0] * Math.PI) / 180));
    return [
      anchor[0] + (offset.north / EARTH_RADIUS_M) * (180 / Math.PI),
      anchor[1] + (offset.east / (EARTH_RADIUS_M * scale)) * (180 / Math.PI),
    ];
  }

  /** Metre offset `{north, east}` from `anchor` to `coordinate`. */
  function coordinateToOffset(anchor, coordinate) {
    const scale = Math.max(0.01, Math.cos((anchor[0] * Math.PI) / 180));
    return {
      north:
        ((coordinate[0] - anchor[0]) * Math.PI / 180) * EARTH_RADIUS_M,
      east:
        ((coordinate[1] - anchor[1]) * Math.PI / 180) *
        EARTH_RADIUS_M *
        scale,
    };
  }

  window.OFMGeometry = {
    haversineMeters,
    routeLengthMeters,
    buildRouteGeometry,
    anchoredGuide,
    findRadiusIssues,
    projectToPolyline,
    splitAtProjection,
    pointAtFraction,
    offsetToCoordinate,
    coordinateToOffset,
  };
})();
