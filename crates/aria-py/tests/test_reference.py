"""Every public name of the module, and every public member of its classes,
appears in api-design.md, the reference the README links to.

A module name counts when the reference holds it as a word; a class member
counts when the reference holds it after a dot, `.name`, the form a code sample
writes. The names come from the module at run time, so a new class, function,
method, or property without a line in the reference fails here. Exception
classes are exempt from the member census: their members are Python's.

Not caught: a name the reference mentions without describing it, and a member
whose `.name` appears only in a sample for another class.
"""

from __future__ import annotations

import re
from pathlib import Path

import aria

REFERENCE = Path(__file__).resolve().parents[1] / "api-design.md"

# The compiled extension module the package re-exports from, not a surface.
NOT_A_SURFACE = {"aria"}


def public(owner: object) -> list[str]:
    return sorted(name for name in dir(owner) if not name.startswith("_"))


def unreferenced(text: str) -> list[str]:
    names = [name for name in public(aria) if name not in NOT_A_SURFACE]
    missing = [name for name in names if not re.search(rf"\b{name}\b", text)]
    for name in names:
        owner = getattr(aria, name)
        if not isinstance(owner, type) or issubclass(owner, BaseException):
            continue
        missing += [
            f"{name}.{member}"
            for member in public(owner)
            if not re.search(rf"\.{member}\b", text)
        ]
    return missing


def test_every_public_name_is_in_the_reference() -> None:
    missing = unreferenced(REFERENCE.read_text())
    assert missing == [], f"api-design.md holds no line for {missing}"


def test_the_census_reads_every_class() -> None:
    # A census over nothing passes; this pins that it reads members at all.
    assert "Clip.set_animation" in unreferenced("")
    assert "animations" in unreferenced("")
