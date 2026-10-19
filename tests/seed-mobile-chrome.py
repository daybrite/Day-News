#!/usr/bin/env python3
"""Make an isolated, offline long-feeds fixture for phone and tablet chrome gestures.

Run after seeding /tmp/day-news-theme-sync-validation with demo feeds and synthetic Feedbin.
Never reads or changes a user's production library.
"""
from pathlib import Path
import sqlite3

source = Path('/tmp/day-news-theme-sync-validation/sheets2.sqlite3')
target = Path('/tmp/day-news-mobile-chrome')
target.mkdir(exist_ok=True)
with sqlite3.connect(f'file:{source}?mode=ro', uri=True) as src:
    with sqlite3.connect(target / 'sheets2.sqlite3') as dst:
        src.backup(dst)
        for index in range(30):
            dst.execute('''INSERT OR REPLACE INTO feeds
                (id, feed_url, title, unsubscribed, poll_failures, position, next_poll_at)
                VALUES (?, ?, ?, 0, 0, ?, ?)''',
                (900000 + index, f'asset:demo/chrome-fixture-{index}.xml',
                 f'Chrome fixture {index + 1:02}', 100 + index, 4102444800))
print('Prepared isolated 30-row scrolling fixture')
