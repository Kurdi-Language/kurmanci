import UIKit
import Kurmanci

class ViewController: UIViewController {
    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .white

        let label = UILabel()
        label.text = "Kurmancî iOS Consumer Ready"
        label.textAlignment = .center
        label.frame = view.bounds
        view.addSubview(label)
    }
}
