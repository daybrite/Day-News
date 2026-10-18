import XCTest

// Real UIKit gestures: injected route changes cannot catch an offstage split column.
// Run only against the isolated English fixtures documented in native-interactions.md.
final class NavigationTests: XCTestCase {
    let app = XCUIApplication(bundleIdentifier: "dev.daybrite.news")

    func visible(_ element: XCUIElement) {
        let expectation = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "hittable == true"), object: element)
        XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: 8), .completed)
    }

    func photograph(_ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testEmptySmartFeeds() {
        app.activate()
        // Today is selected initially. Repeat it after Back to cover same-route activation.
        for title in ["Today", "Today", "All Unread", "Starred", "All Articles", "Today"] {
            let row = app.staticTexts[title].firstMatch
            visible(row)
            row.tap()
            let empty = app.staticTexts[title == "All Unread" ? "All caught up" : "No articles"].firstMatch
            visible(empty)
            photograph(title)
            app.navigationBars.buttons.element(boundBy: 0).tap()
            visible(app.staticTexts["All Articles"].firstMatch)
        }
    }

    func testPullToRefresh() {
        // Pre-subscribe to /progress/known and return to the feeds root. The slow fixture
        // makes a real request observable; no production subscriptions/accounts are touched.
        app.activate()
        visible(app.staticTexts["Known length fixture"].firstMatch)
        func hits() -> Int {
            let done = expectation(description: "fixture stats")
            var count = -1
            URLSession.shared.dataTask(with: URL(string: "http://127.0.0.1:28762/stats")!) { data, _, error in
                if let data, let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                   let hits = object["hits"] as? [String: Int] {
                    count = hits["/progress/known"] ?? 0
                }
                XCTAssertNil(error)
                done.fulfill()
            }.resume()
            wait(for: [done], timeout: 5)
            return count
        }
        let before = hits()
        let start = app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.3))
        let end = app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.8))
        start.press(forDuration: 0.05, thenDragTo: end)
        // XCTest waits for UIKit animation quiescence, which can outlast this request.
        // The Android real-touch regression separately checks duplicate pulls while busy.
        photograph("Pull refresh")
        XCTAssertEqual(hits(), before + 1, "Pull must start exactly one HTTP request")
        sleep(7)
        start.press(forDuration: 0.05, thenDragTo: end)
        XCTAssertEqual(hits(), before + 2, "Completion must allow another pull")
        sleep(7)
        photograph("Pull refresh complete")
        app.staticTexts["All Articles"].firstMatch.tap()
        visible(app.staticTexts["Synthetic progress article"].firstMatch)
    }
}
