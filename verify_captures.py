#!/usr/bin/env python3
"""
Verify that all capture keys in TextMate grammar files are numeric or special cases.

This script analyzes all grammar files in the grammars-themes collection to confirm
that capture group keys follow the expected format:
- Numeric strings: "0", "1", "2", etc.
- Special metadata keys: "comment", "//"

Any non-conforming keys are reported for investigation.
"""

import json
import os
import sys
from pathlib import Path
from typing import Dict, List, Set, Any

def find_capture_objects(data: Any, path: str = "root") -> List[tuple[str, Dict[str, Any]]]:
    """
    Recursively find all capture objects in the grammar data.

    Returns list of (json_path, capture_dict) tuples.
    """
    captures = []

    if isinstance(data, dict):
        # Check for capture objects
        for capture_key in ["captures", "beginCaptures", "endCaptures", "whileCaptures"]:
            if capture_key in data and isinstance(data[capture_key], dict):
                captures.append((f"{path}.{capture_key}", data[capture_key]))

        # Recursively check nested objects
        for key, value in data.items():
            if key in ["patterns", "repository"]:
                if isinstance(value, list):
                    for i, item in enumerate(value):
                        captures.extend(find_capture_objects(item, f"{path}.{key}[{i}]"))
                elif isinstance(value, dict):
                    for subkey, subvalue in value.items():
                        captures.extend(find_capture_objects(subvalue, f"{path}.{key}.{subkey}"))
            elif isinstance(value, (dict, list)):
                captures.extend(find_capture_objects(value, f"{path}.{key}"))

    elif isinstance(data, list):
        for i, item in enumerate(data):
            captures.extend(find_capture_objects(item, f"{path}[{i}]"))

    return captures

def is_valid_capture_key(key: str) -> tuple[bool, str]:
    """
    Check if a capture key is valid according to TextMate grammar spec.

    Returns (is_valid, category) where category is:
    - "numeric" for "0", "1", "2", etc.
    - "special" for "comment", "//"
    - "invalid" for anything else
    """
    # Check for numeric strings
    if key.isdigit():
        return True, "numeric"

    # Check for special metadata keys
    if key in ["comment", "//"]:
        return True, "special"

    # Everything else is invalid
    return False, "invalid"

def analyze_grammar_file(filepath: Path) -> Dict[str, Any]:
    """Analyze a single grammar file for capture key compliance."""
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            data = json.load(f)
    except Exception as e:
        return {
            "error": f"Failed to parse JSON: {e}",
            "captures_found": 0,
            "violations": []
        }

    # Find all capture objects
    capture_objects = find_capture_objects(data)

    violations = []
    numeric_keys = set()
    special_keys = set()

    for location, capture_dict in capture_objects:
        for key in capture_dict.keys():
            is_valid, category = is_valid_capture_key(key)

            if not is_valid:
                violations.append({
                    "location": location,
                    "invalid_key": key,
                    "all_keys": list(capture_dict.keys())
                })
            elif category == "numeric":
                numeric_keys.add(key)
            elif category == "special":
                print(key, filepath)
                special_keys.add(key)

    return {
        "captures_found": len(capture_objects),
        "numeric_keys": sorted(numeric_keys, key=int),
        "special_keys": sorted(special_keys),
        "violations": violations,
        "error": None
    }

def main():
    """Main verification routine."""
    grammars_dir = Path("./grammars-themes/packages/tm-grammars/grammars")

    if not grammars_dir.exists():
        print(f"❌ Grammar directory not found: {grammars_dir}")
        sys.exit(1)

    print("🔍 Verifying capture key formats in TextMate grammars...")
    print(f"📁 Scanning directory: {grammars_dir.absolute()}")
    print()

    # Find all grammar files
    grammar_files = list(grammars_dir.glob("*.json"))

    if not grammar_files:
        print("❌ No .json grammar files found!")
        sys.exit(1)

    print(f"📋 Found {len(grammar_files)} grammar files")
    print()

    # Analyze each file
    total_captures = 0
    total_violations = 0
    all_numeric_keys = set()
    all_special_keys = set()
    files_with_violations = []

    for filepath in sorted(grammar_files):
        result = analyze_grammar_file(filepath)

        if result["error"]:
            print(f"❌ {filepath.name}: {result['error']}")
            continue

        captures_count = result["captures_found"]
        violations_count = len(result["violations"])

        total_captures += captures_count
        total_violations += violations_count
        all_numeric_keys.update(result["numeric_keys"])
        all_special_keys.update(result["special_keys"])

        if violations_count > 0:
            files_with_violations.append((filepath.name, result))
            print(f"⚠️  {filepath.name}: {captures_count} captures, {violations_count} violations")
        else:
            print(f"✅ {filepath.name}: {captures_count} captures, no violations")

    # Summary report
    print()
    print("=" * 60)
    print("📊 VERIFICATION SUMMARY")
    print("=" * 60)
    print(f"Grammar files analyzed: {len(grammar_files)}")
    print(f"Total capture objects found: {total_captures}")
    print(f"Total violations found: {total_violations}")
    print(f"Files with violations: {len(files_with_violations)}")
    print()

    if all_numeric_keys:
        print(f"✅ Numeric keys found: {sorted(all_numeric_keys, key=int)}")

    if all_special_keys:
        print(f"ℹ️  Special keys found: {sorted(all_special_keys)}")

    # Detail violations if any
    if files_with_violations:
        print()
        print("❌ VIOLATIONS DETAILS:")
        print("-" * 40)

        for filename, result in files_with_violations:
            print(f"\n📄 {filename}:")
            for violation in result["violations"]:
                print(f"  Location: {violation['location']}")
                print(f"  Invalid key: '{violation['invalid_key']}'")
                print(f"  All keys in object: {violation['all_keys']}")

        print()
        print("🚨 CONCLUSION: Non-numeric capture keys found!")
        print("   The Captures type implementation needs to handle these cases.")
        sys.exit(1)
    else:
        print()
        print("🎉 CONCLUSION: All capture keys are numeric or special metadata!")
        print("   Safe to implement Captures type that converts string keys to usize.")

        # Show the range of numeric keys for implementation guidance
        if all_numeric_keys:
            numeric_ints = [int(k) for k in all_numeric_keys]
            print(f"   Numeric key range: {min(numeric_ints)} to {max(numeric_ints)}")

if __name__ == "__main__":
    main()