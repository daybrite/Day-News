fn main() {
    // Locales BEFORE the window options: the window title and the macOS App-menu name are
    // user-facing strings and belong in the catalog, not in a literal here.
    dayapp::res::locales::install();
    day::launch(
        day::WindowOptions {
            title: dayapp::res::str::app_title().format(),
            // Three panes need room. At 960 the timeline and the article both end up too
            // narrow to read comfortably; this is close to NetNewsWire's own default.
            size: day::prelude::Size::new(1440.0, 900.0),
            min_size: Some(day::prelude::Size::new(720.0, 480.0)),
            ..Default::default()
        },
        dayapp::root,
    );
}
