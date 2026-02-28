"""Entry point for sqtop."""

from .app import SqtopApp


def main() -> None:
    app = SqtopApp()
    app.run()


if __name__ == "__main__":
    main()
