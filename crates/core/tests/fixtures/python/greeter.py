"""Module docstring."""
import os


class Greeter:
    """Says hello."""

    def greet(self, name):
        # build the greeting
        text = "hi " + name
        logging.info("greeting %s", name)
        try:
            os.write(1, text)
        except OSError as error:
            print(error)
            raise
        return text


def test_greet():
    assert Greeter().greet("a") == "hi a"
