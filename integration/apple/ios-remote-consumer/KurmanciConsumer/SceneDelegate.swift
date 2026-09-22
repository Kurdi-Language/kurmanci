import UIKit
import SwiftUI

class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(
        _ scene: UIScene,
        willConnectTo session: UISceneSession,
        options connectionOptions: UIScene.ConnectionOptions
    ) {
        guard let windowScene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: windowScene)
        // The try-out screen (TryOutView) is the whole app: see docs/ios-tryout.md.
        window.rootViewController = UIHostingController(rootView: TryOutView())
        window.makeKeyAndVisible()
        self.window = window
    }
}
