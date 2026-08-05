#if !canImport(XCTest)
import Foundation

open class XCTestCase {
    public required init() {}
}

public func XCTAssertEqual<T: Equatable>(_ expression1: @autoclosure () throws -> T, _ expression2: @autoclosure () throws -> T, _ message: @autoclosure () -> String = "", file: StaticString = #filePath, line: UInt = #line) {
    do {
        let val1 = try expression1()
        let val2 = try expression2()
        if val1 != val2 {
            print("❌ Failure at \(file):\(line) - Expected '\(val1)' to equal '\(val2)'. \(message())")
            exit(1)
        }
    } catch {
        print("❌ Unexpected error at \(file):\(line): \(error)")
        exit(1)
    }
}

public func XCTAssertTrue(_ expression: @autoclosure () throws -> Bool, _ message: @autoclosure () -> String = "", file: StaticString = #filePath, line: UInt = #line) {
    XCTAssertEqual(try expression(), true, message(), file: file, line: line)
}

public func XCTAssertFalse(_ expression: @autoclosure () throws -> Bool, _ message: @autoclosure () -> String = "", file: StaticString = #filePath, line: UInt = #line) {
    XCTAssertEqual(try expression(), false, message(), file: file, line: line)
}

public func XCTAssertGreaterThan<T: Comparable>(_ expression1: @autoclosure () throws -> T, _ expression2: @autoclosure () throws -> T, _ message: @autoclosure () -> String = "", file: StaticString = #filePath, line: UInt = #line) {
    do {
        let val1 = try expression1()
        let val2 = try expression2()
        if !(val1 > val2) {
            print("❌ Failure at \(file):\(line) - Expected '\(val1)' > '\(val2)'. \(message())")
            exit(1)
        }
    } catch {
        print("❌ Unexpected error at \(file):\(line): \(error)")
        exit(1)
    }
}

public func XCTAssertLessThanOrEqual<T: Comparable>(_ expression1: @autoclosure () throws -> T, _ expression2: @autoclosure () throws -> T, _ message: @autoclosure () -> String = "", file: StaticString = #filePath, line: UInt = #line) {
    do {
        let val1 = try expression1()
        let val2 = try expression2()
        if !(val1 <= val2) {
            print("❌ Failure at \(file):\(line) - Expected '\(val1)' <= '\(val2)'. \(message())")
            exit(1)
        }
    } catch {
        print("❌ Unexpected error at \(file):\(line): \(error)")
        exit(1)
    }
}

public func XCTAssertThrowsError<T>(_ expression: @autoclosure () throws -> T, _ message: @autoclosure () -> String = "", file: StaticString = #filePath, line: UInt = #line, _ errorHandler: (_ error: Error) -> Void = { _ in }) {
    do {
        _ = try expression()
        print("❌ Failure at \(file):\(line) - Expected error to be thrown. \(message())")
        exit(1)
    } catch {
        errorHandler(error)
    }
}

public func XCTFail(_ message: String = "", file: StaticString = #filePath, line: UInt = #line) {
    print("❌ Failure at \(file):\(line) - \(message)")
    exit(1)
}
#endif
