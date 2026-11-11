"""The python blocks of README.md run as written, in order, in one namespace.

A block that raises fails here, and so does an expression statement whose
trailing comment disagrees with what it evaluates to: a comment that parses as
a Python literal must equal the value, and `print(x)  # text` must print
`text`. Comments that are prose are not compared. The files the blocks import
are made in a scratch directory, so a block that reads a new file name fails
until `INPUTS` makes it.

Not caught: claims made in prose, code fenced as anything but `python`, and the
blocks of api-design.md, which cut from footage longer than 13 s that the
suite has no cheap way to make.
"""

from __future__ import annotations

import ast
import io
import re
import shutil
import tokenize
from collections.abc import Callable
from pathlib import Path

import pytest

from test_api import write_solid_png, write_tone_wav

README = Path(__file__).resolve().parents[1] / "README.md"

# The files the blocks import, made on first mention. The quick start's export
# is the footage the later blocks cut from.
INPUTS: dict[str, Callable[[], None]] = {
    "footage.mp4": lambda: shutil.copyfile("out.mp4", "footage.mp4"),
    "photo.png": lambda: write_solid_png("photo.png", 64, 48, (200, 40, 40, 255)),
    "mine.png": lambda: write_solid_png("mine.png", 64, 48, (40, 200, 40, 255)),
    "music.wav": lambda: write_tone_wav("music.wav", seconds=2.0),
}


def python_blocks(text: str) -> list[str]:
    return re.findall(r"^```python\n(.*?)^```", text, re.S | re.M)


def comments_by_line(source: str) -> dict[int, str]:
    tokens = tokenize.generate_tokens(io.StringIO(source).readline)
    return {
        tok.start[0]: tok.string[1:].strip()
        for tok in tokens
        if tok.type == tokenize.COMMENT
    }


def claimed(comment: str) -> tuple[bool, object]:
    """The literal a comment states, if it states one."""
    try:
        return True, ast.literal_eval(comment)
    except (ValueError, SyntaxError):
        return False, None


def run_block(index: int, source: str, ns: dict[str, object]) -> None:
    for name in re.findall(r"import_media\(\"([^\"]+)\"\)", source):
        if not Path(name).exists():
            assert name in INPUTS, f"README block {index} imports {name}; add it to INPUTS"
            INPUTS[name]()
    comments = comments_by_line(source)
    for stmt in ast.parse(source).body:
        where = f"README python block {index}: {ast.get_source_segment(source, stmt)}"
        comment = comments.get(stmt.end_lineno, "")
        call = stmt.value if isinstance(stmt, ast.Expr) else None
        printed = (
            isinstance(call, ast.Call)
            and isinstance(call.func, ast.Name)
            and call.func.id == "print"
            and len(call.args) == 1
        )
        try:
            if printed:
                value = str(eval(compile(ast.Expression(call.args[0]), "README", "eval"), ns))
                assert value == comment, f"{where} prints {value!r}, the comment shows {comment!r}"
            elif call is not None:
                value = eval(compile(ast.Expression(call), "README", "eval"), ns)
                states, literal = claimed(comment)
                if states:
                    assert value == literal, f"{where} is {value!r}, the comment shows {literal!r}"
            else:
                exec(compile(ast.Module([stmt], type_ignores=[]), "README", "exec"), ns)
        except AssertionError:
            raise
        except Exception as err:  # noqa: BLE001
            pytest.fail(f"{where} raised {type(err).__name__}: {err}")


def test_every_readme_block_runs_as_written(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    blocks = python_blocks(README.read_text(encoding="utf-8"))
    assert blocks, f"{README} has no python blocks"
    monkeypatch.chdir(tmp_path)
    ns: dict[str, object] = {}
    for index, source in enumerate(blocks):
        run_block(index, source, ns)


@pytest.mark.parametrize(
    ("block", "problem"),
    [
        ("x = 1\nx  # 2\n", "is 1, the comment shows 2"),
        ("print(3)  # 4\n", "prints '3', the comment shows '4'"),
        ("[][1]\n", "raised IndexError"),
        ("x = 5\nx  # five, as prose\n", None),
    ],
)
def test_a_block_that_disagrees_with_its_comment_fails(block: str, problem: str | None) -> None:
    if problem is None:
        run_block(0, block, {})
        return
    with pytest.raises((AssertionError, pytest.fail.Exception), match=re.escape(problem)):
        run_block(0, block, {})
