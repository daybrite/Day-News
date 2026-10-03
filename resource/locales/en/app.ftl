language_name = English
app_title = Day News
nav_all_unread = All Unread
nav_all_articles = All Articles
nav_starred = Starred
timeline_empty = No articles
timeline_empty_unread = All caught up
reader_empty = Select an article
reader_no_content = This article has no content. Open it in your browser to read it.
search_placeholder = Search articles
subscribe_heading = Add a subscription
subscribe_placeholder = Feed or site URL
subscribe_action = Subscribe
refresh_action = Refresh
toggle_read = Toggle Read/Unread
refresh_progress = Refreshing { $done } of { $total }…
mark_all_read = Mark All as Read
mark_read = Mark as Read
mark_unread = Mark as Unread
star = Star
unstar = Unstar
unsubscribe = Unsubscribe
opml_heading = Subscriptions file
opml_import = Import OPML…
opml_export = Export OPML…
feeds_count =
    { $count ->
        [one] { $count } feed
       *[other] { $count } feeds
    }
nav_today = Today
nav_smart_feeds = Smart Feeds
nav_feeds_section = Feeds
menu_file = File
menu_go = Go
menu_new_feed = New Feed…
menu_new_folder = New Folder…
menu_refresh = Refresh
menu_import = Import Subscriptions…
menu_export = Export Subscriptions…
menu_next_unread = Next Unread
menu_feed = Feed
menu_refresh_feed = Refresh Feed
new_folder_title = New Folder
new_folder_placeholder = Folder name
unread_count = { $n ->
    [0] No unread
    [one] { $n } unread
   *[other] { $n } unread
}
untitled = Untitled
menu_article = Article
menu_mark_read = Mark as Read
menu_mark_unread = Mark as Unread
menu_star = Mark as Starred
menu_unstar = Mark as Unstarred
menu_open_in_browser = Open in Browser
nav_tags_section = Tags
nav_settings = Settings
tag_action = Tag…
tag_prompt_title = Tag Article
tag_prompt_placeholder = Tag name (again to remove)
menu_tag = Tag Article…
settings_heading = Settings
settings_retention_label = Keep articles
settings_retention_note = Read articles older than this are removed at launch. Starred and tagged articles are always kept.
retention_30 = For one month
retention_90 = For three months
retention_180 = For six months
retention_365 = For one year
retention_forever = Forever
retention_pruned = Removed { $n } old articles

time_now = now
time_minutes = { $n }m
time_hours = { $n }h
time_days = { $n }d
timeline_date = { DATETIME($when, dateStyle: "medium", timeStyle: "none") }
article_date = { DATETIME($when, dateStyle: "long", timeStyle: "short") }

settings_browser_label = Open web links in
settings_browser_system = System Default
settings_browser_default = System Default ({ $name })
settings_browser_note = This choice applies to Day News. Other links use their system application.
settings_browser_unavailable = Links use your default browser. Change it in the device’s system settings.

menu_increase_text_size = Increase Text Size
menu_decrease_text_size = Decrease Text Size
settings_reader_heading = Article Reader
settings_reader_size = Text size
settings_reader_percent = { $value }%
settings_reader_font = Font
settings_reader_default_font = System Font
settings_reader_font_note = Recommended fonts appear first, followed by all available fonts.
settings_reader_background = Background color
settings_reader_text = Text color
settings_reader_reset = Reset Styles
settings_reader_reset_note = Restore System appearance, two preview lines, and default list and reader sizes, reader font, and colors.
settings_appearance = Appearance
settings_light = Light
settings_dark = Dark
settings_system = System

settings_list_heading = Article List
settings_preview_lines = Preview lines
settings_preview_none = No preview
settings_preview_lines_count = { $count ->
    [one] 1 line
   *[other] { $count } lines
    }

settings_list_size = Text size
settings_list_percent = { $value }%
menu_reader_view = Reader View
reader_view_loading = Loading full article…
reader_view_network_error = Couldn’t load the full article. Try Reader View again or open the original website.
reader_view_too_large = This page is too large for Reader View. Open the original website to read it.
reader_view_no_content = No readable article was found. Open the original website to read it.
reader_view_timeout = Loading the full article took too long. Try Reader View again.
reader_view_unavailable = Reader View is unavailable. Open the original website to read the full article.
show_unread_feeds_only = Show Only Unread Feeds

menu_find_articles = Find Articles…
storage_error = { $error }

status_feed_refreshed = Refreshed { $title }
status_feed_failed = Could not refresh { $title }
status_feeds_refreshed = Refreshed { $done } of { $total } feeds — { $failed } failed
status_imported = Imported { $added } feeds ({ $existing } already subscribed)
subscriptions_export_title = Day News Subscriptions
menu_previous_article = Previous Article
menu_next_article = Next Article
menu_copy_article_link = Copy Article Link
copy_link_failed = The article link could not be copied. Please try again.
dismiss_alert = OK

settings_refresh_feeds = Refresh Feeds
settings_refresh_manual = Manually
settings_refresh_30 = Every 30 Minutes
settings_refresh_60 = Every Hour
settings_refresh_120 = Every 2 Hours
settings_refresh_240 = Every 4 Hours
settings_refresh_480 = Every 8 Hours

settings_refresh_automatic = Automatic
settings_refresh_automatic_note = Automatic checks each feed based on how often and how recently it publishes, from every 30 minutes to once a day.
group_by_feeds = Group by Feeds
move_feed_up = Move Feed Up
move_feed_down = Move Feed Down
feed_order_note = Reorder feeds to set refresh priority and article group order. Drag feeds or use Move Feed Up and Move Feed Down.

# Dashboards. Numbers, dates, chart axes and countdown digits follow the active locale.
dashboard_overview = FEED INTELLIGENCE
dashboard_today_eyebrow = YOUR DAY IN STORIES
dashboard_unread_eyebrow = YOUR READING HORIZON
dashboard_starred_eyebrow = YOUR PERSONAL COLLECTION
dashboard_library_eyebrow = THE BIG PICTURE
dashboard_feed_intro = A portrait of this publication, built from the articles in your library.
dashboard_today_intro = What arrived today, who published it, and how much there is to explore.
dashboard_unread_intro = See the shape of your backlog and find room for your next good read.
dashboard_starred_intro = The stories you kept: a map of your interests, publishers and reading time.
dashboard_library_intro = Your publications, their rhythms, and the stories they bring together.
dashboard_articles = Articles
dashboard_unread = Unread
dashboard_reading = Reading time
dashboard_publishers = Publishers
dashboard_read = Read
dashboard_number = { NUMBER($value, maximumFractionDigits: 0) }
dashboard_minutes = { NUMBER($value, maximumFractionDigits: 0) } min
dashboard_percent = { NUMBER($value, maximumFractionDigits: 0) }%
dashboard_words = { NUMBER($value, maximumFractionDigits: 0) } words on average
dashboard_authors = { $count ->
    [one] One contributing author
   *[other] { NUMBER($count) } contributing authors
    }
dashboard_activity = Publication rhythm
dashboard_activity_today = Arrivals by hour
dashboard_activity_unread = Age of your unread stories
dashboard_activity_starred = When your saved stories were published
dashboard_activity_note = { DATETIME($start, dateStyle: "medium", timeStyle: "none") } – { DATETIME($end, dateStyle: "medium", timeStyle: "none") } · articles by publication date
dashboard_today_note = Today, in your local time
dashboard_unread_note = Recent discoveries and stories waiting a little longer
dashboard_lengths = Story lengths
dashboard_lengths_note = Estimated words · stored article text
dashboard_sources = Publisher mix
dashboard_sources_note = Where these stories come from
dashboard_chart_date = { DATETIME($when, dateStyle: "medium", timeStyle: "none") }
dashboard_chart_hour = { NUMBER($hour, minimumIntegerDigits: 2, useGrouping: "false") }:00
dashboard_chart_count = Articles
dashboard_chart_words = Words
dashboard_short = <100
dashboard_medium = 100–299
dashboard_long = 300–699
dashboard_deep = 700–1,499
dashboard_very_long = 1,500+
dashboard_age_today = <1 day
dashboard_age_week = 1–6 days
dashboard_age_month = 1–4 weeks
dashboard_age_quarter = 1–3 months
dashboard_age_old = 3+ months
dashboard_other = Other publishers
dashboard_empty = No stories here yet
dashboard_empty_note = Your dashboard will take shape as articles arrive.
dashboard_loading = Gathering your feed insights…
dashboard_automatic = AUTOMATIC REFRESH
dashboard_every_minutes = Every { NUMBER($minutes) } minutes
dashboard_every_hours = Every { NUMBER($hours) } hours
dashboard_daily = Once a day
dashboard_interval_range = { $fast } – { $slow }
dashboard_next = Next check: { DATETIME($when, dateStyle: "medium", timeStyle: "medium") }
dashboard_countdown = { NUMBER($hours, minimumIntegerDigits: 2, useGrouping: "false") }:{ NUMBER($minutes, minimumIntegerDigits: 2, useGrouping: "false") }:{ NUMBER($seconds, minimumIntegerDigits: 2, useGrouping: "false") }
dashboard_due = Due now · waiting for the next scheduler tick
dashboard_refreshing = Checking feeds now…
dashboard_manual = Manual refresh is selected. Automatic would use this cadence.
dashboard_fixed = A fixed refresh interval is selected. Automatic would use this cadence.
dashboard_no_schedule = Subscribe to a feed to see its refresh forecast.
dashboard_backoff = Some feeds are backing off after an error or an origin-requested delay.
dashboard_policy = Based on recent publication cadence and recency; quiet feeds are checked less often.
dashboard_last_story = Latest story: { DATETIME($when, dateStyle: "medium", timeStyle: "short") }
dashboard_show = Feed overview
dashboard_habits = Publishing habits
dashboard_habits_note = Local weekday and hour · brighter means more stories
dashboard_mon = Mon
dashboard_tue = Tue
dashboard_wed = Wed
dashboard_thu = Thu
dashboard_fri = Fri
dashboard_sat = Sat
dashboard_sun = Sun
dashboard_checked = Last checked: { DATETIME($when, dateStyle: "medium", timeStyle: "short") }
dashboard_read_share = { NUMBER($percent, maximumFractionDigits: 0) }% read · { NUMBER($count) } starred
dashboard_contributors = Named authors

dashboard_refresh_now = Refresh Now

dashboard_cell_annotation = { NUMBER($hour, minimumIntegerDigits: 2, useGrouping: "false") }:00 · { $count ->
    [one] 1 article
   *[other] { NUMBER($count, maximumFractionDigits: 0) } articles
    }

subscribe_invalid = Enter a valid website or feed URL
subscribe_missing = No feed found
subscribe_missing_note = This website does not advertise an RSS, Atom, or JSON feed.
subscribe_choose = Choose a feed
subscribe_choose_note = This website offers several feeds. Select the one you want to follow.
subscribe_candidate = { $title } — { $url }
subscribe_failed = Could not subscribe to this feed. Check the URL and try again.
cancel_action = Cancel
opml_filter = Subscription lists
opml_import_failed = Import failed: { $error }
opml_read_failed = Could not read the file: { $error }
opml_exported = Exported subscriptions
