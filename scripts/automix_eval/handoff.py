"""Descriptive handoff measurements. None is a musical-quality verdict."""
def power(bands):
    return sum(map(sum, bands)) if bands is not None else None


def active_source(track, threshold):
    cache = getattr(track, '_active_source_cache', {})
    if threshold in cache:
        return cache[threshold]
    active = [(f['start_sec'], min(f['end_sec'], track.duration))
              for f, bands in zip(track.frames, track.powers) if power(bands) > threshold]
    missing = 0.
    end = active[-1][1] if active else None
    if end is not None:
        cursor = end
        for f in track.frames:
            if f['end_sec'] <= end:
                continue
            missing += max(0., f['start_sec'] - cursor)
            cursor = max(cursor, f['end_sec'])
        missing += max(0., track.duration - cursor)
    value = {'status': 'observed' if end is not None and missing < 1e-6 else 'unknown',
             'first_active_source_seconds': active[0][0] if active else None,
             'last_active_source_seconds': end, 'unknown_tail_seconds': missing,
             'intervals': active}
    cache[threshold] = value
    track._active_source_cache = cache
    return value


def runs(rows):
    result = []
    for lo, hi, state in rows:
        if state is not True:
            continue
        if result and abs(result[-1][1] - lo) < 1e-7:
            result[-1][1] = hi
        else:
            result.append([lo, hi])
    return result


def summarize(plan, rows, threshold):
    duration = plan.mix_end
    timeline = []
    last = None
    first_b = None
    last_tonal = None
    lower_sec = lower_bars = unknown_sec = unknown_bars = 0.
    for row in rows:
        wall, dt, decks = row['wall'], row['dt'], row['decks']
        for side, d in enumerate(decks):
            value = power(d['heard'])
            if value is not None and value > threshold and d['playing']:
                if not side and wall < duration:
                    last = {'wall_seconds': wall, 'source_seconds': d['source']}
                elif side and first_b is None:
                    first_b = {'wall_seconds': wall, 'source_seconds': d['source']}
            if not side and d['playing'] and d['heard'] and wall < duration:
                tonal = sum(sum(d['heard'][part]) for part in (0, 2, 3))
                if tonal > threshold:
                    last_tonal = {'wall_seconds': wall, 'source_seconds': d['source']}
        lo, hi = max(0., wall - dt / 2), min(duration, wall + dt / 2)
        if hi <= lo:
            continue
        values = [power(d['heard']) for d in decks]
        state = (False if not all(d['playing'] for d in decks) else
                 None if any(v is None for v in values) else all(v > threshold for v in values))
        bars = plan.inverse.at(hi) - plan.inverse.at(lo)
        if state is None:
            unknown_sec += hi - lo
            unknown_bars += bars
        elif state:
            lower_sec += hi - lo
            lower_bars += bars
        timeline.append((lo, hi, state))
    if plan.unsupported:
        unknown_sec, unknown_bars = duration, plan.mix_bar
    segments = runs(timeline)
    # This descriptor bridges only short observed rests, never missing evidence.
    # Half/double grid aliases produce the same normalized pulse; weak grids
    # leave the descriptor unavailable instead of guessing a tempo.
    rate = plan.curves[0]['rate'].at(0)
    grid = plan.a.grid(plan.data['t_in_a'], 1.)
    gap = None
    if grid:
        pulse = grid[0]
        while pulse < .375:
            pulse *= 2
        while pulse >= .75:
            pulse /= 2
        gap = min(.25, pulse / (2 * max(.01, rate)))
    joined = []
    for lo, hi in segments:
        known_gap = joined and not any(state is None and a < lo and b > joined[-1][1]
                                      for a, b, state in timeline)
        if joined and gap is not None and lo - joined[-1][1] <= gap + 1e-8 and known_gap:
            joined[-1][1] = hi
        else:
            joined.append([lo, hi])
    source = active_source(plan.a, threshold)
    raw_end = source['last_active_source_seconds']
    heard = last['source_seconds'] if last else None
    remaining = raw_end - heard if heard is not None and raw_end is not None else None
    # Keep the historical rule explicitly labeled. A missing observation must
    # never turn into a zero-overlap terminal flag.
    nominal_late = plan.data['t_out_a'] >= min(plan.a.duration * .92, plan.a.duration - 8)
    observed = not plan.unsupported and unknown_sec < 1e-6
    short = lower_bars < 4 - 1e-8 if observed else None
    legacy = (short if nominal_late else False) if not plan.unsupported else None
    tail = None
    if remaining is not None and source['status'] == 'observed':
        span = raw_end - source['first_active_source_seconds']
        tail = remaining <= max(8., span * .08)
    return {
        'status': 'unavailable' if plan.unsupported else 'observed' if observed else 'partial',
        'reason': plan.unsupported,
        'legacy_tail_short': legacy,
        'nominal_remaining_file_seconds': plan.a.duration - plan.data['t_out_a'],
        'outgoing_last_heard': last, 'outgoing_last_tonal': last_tonal,
        'incoming_first_heard': first_b,
        'heard_endpoint_status': 'observed_in_window' if last else 'unknown',
        'source_tail': {k: v for k, v in source.items() if k != 'intervals'},
        'remaining_active_source_seconds': sum(max(0., hi - max(lo, heard))
            for lo, hi in source['intervals']) if heard is not None else None,
        'remaining_to_active_end_seconds': remaining,
        'heard_tail_position': tail,
        'overlap': {
            'eligible_seconds': duration, 'observed_seconds': max(0., duration - unknown_sec),
            'unknown_seconds': unknown_sec,
            'seconds_lower': lower_sec, 'seconds_upper': lower_sec + unknown_sec,
            'bars_lower': lower_bars, 'bars_upper': lower_bars + unknown_bars,
            'under_four_clock_bars': short,
            'runs_wall_seconds': segments,
            'longest_run_seconds': max((b - a for a, b in segments), default=0.) if observed else None,
            'gap_tolerance_seconds': gap,
            'gap_tolerant_longest_seconds': max((b - a for a, b in joined), default=0.)
            if observed and gap is not None else None,
        },
    }


def aggregate(pairs):
    hs = [p['handoff'] for p in pairs]
    return {
        'legacy_tail_short_candidates': sum(h['legacy_tail_short'] is True for h in hs),
        'legacy_tail_unknown_pairs': sum(h['legacy_tail_short'] is None for h in hs),
        'heard_tail_position_pairs': sum(h['heard_tail_position'] is True for h in hs),
        'unknown_heard_endpoint_pairs': sum(h['outgoing_last_heard'] is None for h in hs),
        'unavailable_pairs': sum(h['status'] == 'unavailable' for h in hs),
        'overlap_unknown_seconds': sum(h['overlap']['unknown_seconds'] for h in hs),
        'overlap_seconds_lower': sum(h['overlap']['seconds_lower'] for h in hs),
        'overlap_seconds_upper': sum(h['overlap']['seconds_upper'] for h in hs),
    }
