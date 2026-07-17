"""Typed public errors raised by the Python SDK."""


class ContractError(ValueError):
    """A value did not match its Rust-owned SDK contract."""


class OneHarnessProcessError(RuntimeError):
    """The oneharness subprocess exited unsuccessfully."""

    def __init__(self, returncode: int, stderr: str) -> None:
        self.returncode = returncode
        self.stderr = stderr
        super().__init__(f"oneharness exited {returncode}: {stderr.strip()}")


class HistoryNotFoundError(OneHarnessProcessError):
    """A history session, record, or watch cursor could not be resolved."""
