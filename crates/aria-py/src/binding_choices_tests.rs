//! WHY: closes the class of a script error that names a bad key and stops. An
//! unknown effect parameter, text style key or shape style key named only the
//! key, so a script that guessed `font_size` for `size` had to read the source
//! to find the right one.
//!
//! The script reads every effect off `aria.effects()` and every listed style key
//! off the error itself, so a new effect, effect parameter or shape parameter is
//! covered without a row here. Each listed name must be one the same call
//! accepts. Not caught: a text style key that the match reads and the list
//! omits, since the list is the only record of the keys.

use std::ffi::CString;

use pyo3::prelude::*;
use pyo3::types::PyDict;

/// Runs `code` with `aria` bound and returns the `str()` of its `result`.
fn result_of(code: &str) -> String {
    Python::attach(|py| {
        let globals = PyDict::new(py);
        globals
            .set_item("aria", pyo3::wrap_pymodule!(crate::aria)(py))
            .expect("binding a module into a fresh dict cannot fail");
        let script = CString::new(code).expect("no NUL in the script");
        py.run(&script, Some(&globals), None)
            .unwrap_or_else(|err| panic!("the script raised {}", err.value(py)));
        globals
            .get_item("result")
            .expect("reading a dict cannot fail")
            .expect("the script sets result")
            .to_string()
    })
}

#[test]
fn an_unknown_effect_parameter_lists_the_effects_parameters() {
    let wrong = result_of(
        "p = aria.Project('choices', fps=30)\n\
         c = p.add_track('sticker').add(aria.Rect(), 0.0, 2.0)\n\
         result = []\n\
         for e in aria.effects():\n    \
             fx = c.add_effect(e['id'])\n    \
             names = [q['name'] for q in e['params']]\n    \
             for name in names:\n        \
                 fx[name]\n    \
             if names:\n        \
                 want = 'unknown effect parameter \"nope\"; available %s parameters: %s' \
                     % (e['id'], ', '.join(names))\n    \
             else:\n        \
                 want = 'unknown effect parameter \"nope\"; %s has no parameters' % e['id']\n    \
             try:\n        \
                 fx['nope']\n        \
                 result.append(e['id'] + ' read nope')\n    \
             except ValueError as err:\n        \
                 if str(err) != want:\n            \
                     result.append(str(err))\n    \
             try:\n        \
                 fx['nope'] = 1.0\n        \
                 result.append(e['id'] + ' wrote nope')\n    \
             except ValueError as err:\n        \
                 if str(err) != want:\n            \
                     result.append(str(err))\n",
    );
    assert_eq!(wrong, "[]");
}

#[test]
fn an_unknown_style_key_lists_every_key_set_style_takes() {
    let wrong = result_of(
        "p = aria.Project('choices', fps=30)\n\
         clips = {\n    \
             'text': p.add_track('text').add(aria.Text('hi'), 0.0, 2.0),\n    \
             'shape': p.add_track('sticker').add(aria.Rect(), 0.0, 2.0),\n\
         }\n\
         result = []\n\
         for kind, c in clips.items():\n    \
             head = 'unknown %s style parameter \"nope\"; available %s style parameters: ' \
                 % (kind, kind)\n    \
             try:\n        \
                 c.set_style(nope=1)\n        \
                 result.append(kind + ' took nope')\n        \
                 continue\n    \
             except ValueError as err:\n        \
                 why = str(err)\n    \
             if not why.startswith(head):\n        \
                 result.append(why)\n        \
                 continue\n    \
             keys = why[len(head):].split(', ')\n    \
             if len(keys) < 3:\n        \
                 result.append(kind + ' lists ' + why)\n    \
             for key in keys:\n        \
                 if not key.isidentifier():\n            \
                     result.append(kind + ' lists ' + key)\n        \
                 try:\n            \
                     c.set_style(**{key: object()})\n        \
                 except Exception as err:\n            \
                     if str(err).startswith('unknown'):\n                \
                         result.append(str(err))\n",
    );
    assert_eq!(wrong, "[]");
}
