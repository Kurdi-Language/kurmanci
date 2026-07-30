# iOS Custom Keyboard Extension Setup

## Overview

Because Apple does not ship a native Kurmancî autocorrect language model, a custom keyboard extension target must implement candidate lookup and text replacement via `UIInputViewController` and `textDocumentProxy`.

```swift
import UIKit
import KurmanciEngine

final class KeyboardViewController: UIInputViewController {
    private let engine = Engine()

    override func viewDidLoad() {
        super.viewDidLoad()
        setupSuggestionBar()
    }

    func typeCharacter(_ char: String) {
        textDocumentProxy.insertText(char)
        updateSuggestions()
    }

    func pressSpace() {
        applyAutocorrect()
        textDocumentProxy.insertText(" ")
        updateSuggestions()
    }

    private func wordBeforeCursor() -> String {
        guard let context = textDocumentProxy.documentContextBeforeInput else { return "" }
        return context
            .split(whereSeparator: { $0.isWhitespace || $0.isPunctuation })
            .last
            .map(String.init) ?? ""
    }

    private func updateSuggestions() {
        let currentWord = wordBeforeCursor()
        let suggestions = engine.suggest(currentWord, limit: 3)
        suggestionBar.update(with: suggestions)
    }
}
```
