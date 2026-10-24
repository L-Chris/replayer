"""Validate local/published release notes without third-party dependencies."""
import argparse
import json
from pathlib import Path
import re


def validate(body: str) -> None:
    marker = "<summary>中文更新说明</summary>"
    if "\ufffd" in body or re.search(r"\?{3,}", body):
        raise ValueError("Release notes contain damaged text")
    if body.count("<details>") != 1 or body.count(marker) != 1 or body.count("</details>") != 1:
        raise ValueError("Expected one Chinese translation block with the exact summary marker")
    english, translation = body.split("<details>", 1)
    if not translation.lstrip().startswith(marker):
        raise ValueError("The Chinese summary must start the details block")
    chinese, trailing = translation.split(marker, 1)[1].split("</details>", 1)
    if trailing.strip() or re.search(r"[\u3400-\u9fff]", english):
        raise ValueError("Keep English before details and Chinese inside details")
    if not re.search(r"[\u3400-\u9fff]", chinese):
        raise ValueError("Chinese translation is missing or damaged")
    expected = ["Feature", "Improvement", "Fix"]
    sections = []
    for text in (english, chinese):
        headings = re.findall(r"^## (.+)$", text, re.M)
        if not headings or headings != [h for h in expected if h in headings]:
            raise ValueError("Categories must be Feature, Improvement, Fix in order, without duplicates")
        parts = re.split(r"^## .+$", text, flags=re.M)[1:]
        counts = [len(re.findall(r"^- \S.+$", part, re.M)) for part in parts]
        if any(count == 0 for count in counts):
            raise ValueError("Omit categories with no entries")
        sections.append((headings, counts))
    if sections[0] != sections[1]:
        raise ValueError("English and Chinese categories and item counts must match")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--file", type=Path)
    parser.add_argument("--published", type=Path)
    parser.add_argument("--allow-legacy", action="store_true")
    args = parser.parse_args()
    if not re.fullmatch(r"v\d+\.\d+\.\d+", args.tag):
        parser.error("A stable version tag with lowercase v is required")
    local = args.file.read_text(encoding="utf-8") if args.file else None
    if local is not None:
        validate(local)
    if args.published:
        release = json.loads(args.published.read_text(encoding="utf-8"))
        if release["name"] != args.tag or release["tagName"] != args.tag:
            raise ValueError("Published title and tag must equal the requested tag")
        if not args.allow_legacy or local is not None:
            validate(release["body"])
        elif "\ufffd" in release["body"]:
            raise ValueError("Published legacy notes contain replacement characters")
        if local is not None and release["body"].strip() != local.strip():
            raise ValueError("Published body differs from the local notes")
    elif local is None:
        parser.error("Provide --file or --published")
    print(f"Release notes verified: {args.tag}")


if __name__ == "__main__":
    main()
