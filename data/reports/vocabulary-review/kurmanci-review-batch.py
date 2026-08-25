#!/usr/bin/env python3

import csv
import json
import os
import re
import sys
from datetime import date
from pathlib import Path


TSV = Path("data/reports/vocabulary-review/top-1000.tsv")

DECISIONS = Path(
    "data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl"
)

SOURCE_ID = "kurdish-hunspell-kmr"

VALID_STATUSES = {
    "approved",
    "approved_with_metadata_change",
    "rejected_from_default_pack",
    "experimental_only",
    "needs_linguist",
    "needs_source_investigation",
}

STATUS_ALIASES = {
    "rejected": "rejected_from_default_pack",
    "rejected_from_default": "rejected_from_default_pack",
    "rejected-from-default-pack": "rejected_from_default_pack",
    "needs_linguists": "needs_linguist",
    "needs-linguist": "needs_linguist",
    "needs-linguists": "needs_linguist",
    "experimental": "experimental_only",
    "experimental-only": "experimental_only",
    "approved_with_metadata": "approved_with_metadata_change",
    "approved-with-metadata-change": "approved_with_metadata_change",
    "needs_source": "needs_source_investigation",
    "needs-source-investigation": "needs_source_investigation",
}


def die(message):
    raise SystemExit(f"ERROR: {message}")


def markdown_unescape(text):
    """
    Remove backslash escaping before common Markdown punctuation.
    """

    return re.sub(
        r"\\([\\`*_{}\[\]()#+\-.!>:])",
        r"\1",
        text,
    )


def clean_line(line):
    """
    Turn both normal and escaped worksheet lines into a simple form.

    For example, an escaped review line becomes:

        - human decision: needs_linguist
    """

    line = markdown_unescape(line)
    line = line.strip()

    # Remove Markdown bold markers regardless of whether they appear
    # doubled because of previous escaping.
    line = line.replace("**", "")
    line = line.replace("*", "")

    return line.strip()


def clean_value(value):
    value = markdown_unescape(value)
    value = value.strip()

    if (
        len(value) >= 2
        and value.startswith("`")
        and value.endswith("`")
    ):
        value = value[1:-1]

    return value.strip()


def normalize_status(value):
    value = clean_value(value).lower()
    value = STATUS_ALIASES.get(value, value)

    if value not in VALID_STATUSES:
        die(
            f"unsupported review status: {value!r}\n"
            f"Allowed: {', '.join(sorted(VALID_STATUSES))}"
        )

    return value


def load_existing_decisions():
    records = []
    existing_map = {}

    if not DECISIONS.exists():
        return records, existing_map

    lines = DECISIONS.read_text(
        encoding="utf-8"
    ).splitlines()

    for line_no, line in enumerate(lines, start=1):
        if not line.strip():
            continue

        try:
            record = json.loads(line)
        except json.JSONDecodeError as exc:
            die(
                f"invalid JSON already present at "
                f"{DECISIONS}:{line_no}: {exc}"
            )

        records.append(record)

        target_id = record.get("target_id")

        if target_id:
            existing_map[target_id] = record

    return records, existing_map


def export_batch(start, end):
    if not TSV.exists():
        die(f"missing source file: {TSV}")

    if start < 1:
        die("start rank must be >= 1")

    if end < start:
        die("end rank must be >= start rank")

    rows = []

    with TSV.open(
        encoding="utf-8",
        newline="",
    ) as f:
        reader = csv.DictReader(
            f,
            delimiter="\t",
        )

        for row in reader:
            try:
                rank = int(row["rank"])
            except (KeyError, ValueError):
                die(
                    "top-1000.tsv has an invalid "
                    "or missing rank column"
                )

            if start <= rank <= end:
                rows.append(row)

    expected = end - start + 1

    if len(rows) != expected:
        die(
            f"expected {expected} rows for ranks "
            f"{start}-{end}, found {len(rows)}"
        )

    output = Path(
        f"/tmp/kurmanci-review-{start:03d}-{end:03d}.md"
    )

    with output.open(
        "w",
        encoding="utf-8",
    ) as f:
        f.write(
            f"# Kurmancî vocabulary review — "
            f"ranks {start}-{end}\n\n"
        )

        f.write(
            "> Fill only `human decision`, "
            "`review notes`, and `evidence`.\n>\n"
            "> Allowed decisions: "
            "approved, "
            "approved_with_metadata_change, "
            "rejected_from_default_pack, "
            "experimental_only, "
            "needs_linguist, "
            "needs_source_investigation.\n\n"
        )

        for row in rows:
            rank = row["rank"]
            form = row["form"]
            target_id = row["target_id"]

            pos = (
                row.get("part_of_speech", "").strip()
                or "—"
            )

            morphology = (
                row.get("morphology", "").strip()
                or "—"
            )

            tokens = (
                row.get("token_count", "").strip()
                or "0"
            )

            documents = (
                row.get("document_count", "").strip()
                or "0"
            )

            zipf = (
                row.get("zipf", "").strip()
                or "—"
            )

            audit = (
                row.get("audit_flags", "").strip()
                or "none"
            )

            source_lines = (
                row.get("source_lines", "").strip()
                or "—"
            )

            f.write(
                f"## {rank}. {form}\n\n"
                f"- target_id: `{target_id}`\n"
                f"- POS: `{pos}`\n"
                f"- morphology: `{morphology}`\n"
                f"- corpus: {tokens} tokens / "
                f"{documents} documents / Zipf {zipf}\n"
                f"- Hunspell source line: {source_lines}\n"
                f"- audit flags: `{audit}`\n"
                f"- **human decision:**\n"
                f"- **review notes:**\n"
                f"- **evidence:**\n\n"
            )

    print()
    print(f"Wrote {len(rows)} candidates:")
    print(output)
    print()


def parse_heading(line):
    """
    Return (rank, form) when line is a review heading.
    """

    line = clean_line(line)

    # Handle headings such as:
    # ## 101. word
    # # # 101. word
    # ## 101. word **
    line = line.strip()

    match = re.match(
        r"^#{1,3}\s*(\d+)\.\s*(.+?)\s*$",
        line,
    )

    if not match:
        return None

    rank = int(match.group(1))
    form = match.group(2).strip()

    return rank, form


def parse_key_value(line):
    """
    Parse a worksheet bullet line into (key, value).

    Examples:

        - target_id: `abc`
        - human decision: approved
        - review notes:
    """

    line = clean_line(line)

    if not line:
        return None

    if line.startswith("-"):
        line = line[1:].strip()

    if ":" not in line:
        return None

    key, value = line.split(":", 1)

    key = clean_value(key).lower()
    value = clean_value(value)

    return key, value


def parse_review_markdown(path):
    raw_lines = path.read_text(
        encoding="utf-8"
    ).splitlines()

    entries = []

    current = None

    def finish_current():
        nonlocal current

        if current is None:
            return

        if not current.get("status_raw"):
            rank = current.get("rank", "?")
            form = current.get("form", "?")
            die(
                f"rank {rank} ({form}): missing or empty 'human decision' in worksheet {path}"
            )

        required = (
            "rank",
            "form",
            "target_id",
        )

        missing = [
            key
            for key in required
            if not current.get(key)
        ]

        if missing:
            rank = current.get("rank", "?")
            form = current.get("form", "?")

            die(
                f"rank {rank} ({form}): "
                f"missing {', '.join(missing)}"
            )

        current["status"] = normalize_status(
            current["status_raw"]
        )

        entries.append(current)

        current = None

    for raw_line in raw_lines:
        heading = parse_heading(raw_line)

        if heading:
            finish_current()

            rank, form = heading

            current = {
                "rank": rank,
                "form": form,
                "target_id": "",
                "status_raw": "",
                "notes": "",
                "evidence": "",
            }

            continue

        if current is None:
            continue

        parsed = parse_key_value(raw_line)

        if not parsed:
            continue

        key, value = parsed

        if key == "target_id":
            current["target_id"] = value

        elif key == "human decision":
            current["status_raw"] = value

        elif key == "review notes":
            current["notes"] = value

        elif key == "evidence":
            current["evidence"] = value

    finish_current()

    if not entries:
        die(
            f"no review entries found in {path}"
        )

    # Verify ranks are unique.
    seen_ranks = set()

    for entry in entries:
        rank = entry["rank"]

        if rank in seen_ranks:
            die(
                f"duplicate rank in worksheet: {rank}"
            )

        seen_ranks.add(rank)

    # Verify target IDs are unique.
    seen_ids = set()

    for entry in entries:
        target_id = entry["target_id"]

        if target_id in seen_ids:
            die(
                "duplicate target_id inside worksheet: "
                f"{target_id}"
            )

        seen_ids.add(target_id)

    return entries


def validate_entries_before_write(entries):
    """
    Validate the entire worksheet before decisions.jsonl
    can be modified.
    """

    for entry in entries:
        status = entry["status"]
        notes = entry["notes"]
        evidence = entry["evidence"]

        if status == "rejected_from_default_pack":
            if not notes and not evidence:
                die(
                    f"rank {entry['rank']} "
                    f"({entry['form']}): "
                    "rejected_from_default_pack requires "
                    "notes or evidence describing the rejection"
                )

        if status == "approved_with_metadata_change":
            if not notes and not evidence:
                die(
                    f"rank {entry['rank']} "
                    f"({entry['form']}): "
                    "approved_with_metadata_change requires "
                    "notes or evidence describing the change"
                )


def import_batch(path):
    reviewer_id = os.environ.get(
        "REVIEWER_ID",
        "",
    ).strip()

    if not reviewer_id:
        die(
            "REVIEWER_ID is required"
        )

    if not path.exists():
        die(
            f"review file does not exist: {path}"
        )

    entries = parse_review_markdown(path)

    validate_entries_before_write(entries)

    _, existing_map = load_existing_decisions()

    new_entries = []
    skipped_identical = 0

    for entry in entries:
        target_id = entry["target_id"]
        if target_id in existing_map:
            existing_record = existing_map[target_id]
            existing_status = existing_record.get("review_status", "")
            if entry["status"] != existing_status:
                die(
                    f"rank {entry['rank']} ({entry['form']}): conflicting decision status for target_id '{target_id}': "
                    f"worksheet status '{entry['status']}' conflicts with stored decisions.jsonl status '{existing_status}'"
                )

            existing_notes = existing_record.get("review_notes", "")
            incoming_notes = entry["notes"]
            if incoming_notes != existing_notes:
                die(
                    f"rank {entry['rank']} ({entry['form']}): conflicting review notes for target_id '{target_id}': "
                    f"worksheet notes '{incoming_notes}' conflict with stored decisions.jsonl notes '{existing_notes}'"
                )

            existing_evidence = existing_record.get("evidence", [])
            incoming_evidence = [entry["evidence"]] if entry["evidence"] else []
            if incoming_evidence != existing_evidence:
                die(
                    f"rank {entry['rank']} ({entry['form']}): conflicting evidence for target_id '{target_id}': "
                    f"worksheet evidence {incoming_evidence} conflicts with stored decisions.jsonl evidence {existing_evidence}"
                )

            skipped_identical += 1
        else:
            new_entries.append(entry)

    if skipped_identical > 0:
        print(
            f"Verified and skipped {skipped_identical} "
            "decisions already identically present in decisions.jsonl"
        )

    if not new_entries:
        print("No new review decisions to append.")
        return

    entries = new_entries

    review_date = date.today().isoformat()

    records = []

    for entry in entries:
        record = {
            "schema_version": "review-decision-v1",
            "target_type": "entry",
            "target_id": entry["target_id"],
            "source_id": SOURCE_ID,
            "review_status": entry["status"],
            "reviewer_id": reviewer_id,
            "review_date": review_date,
        }

        if entry["notes"]:
            record["review_notes"] = (
                entry["notes"]
            )

        if entry["evidence"]:
            record["evidence"] = [
                entry["evidence"]
            ]

        records.append(record)

    # Everything has been parsed and validated.
    # Only now modify decisions.jsonl.

    needs_newline = False

    if (
        DECISIONS.exists()
        and DECISIONS.stat().st_size > 0
    ):
        with DECISIONS.open("rb") as f:
            f.seek(-1, 2)

            needs_newline = (
                f.read(1) != b"\n"
            )

    with DECISIONS.open("ab") as f:
        if needs_newline:
            f.write(b"\n")

        for record in records:
            line = json.dumps(
                record,
                ensure_ascii=False,
                separators=(",", ":"),
            )

            f.write(
                (line + "\n").encode("utf-8")
            )

    counts = {}

    for entry in entries:
        status = entry["status"]

        counts[status] = (
            counts.get(status, 0) + 1
        )

    print()
    print(
        f"Appended {len(records)} "
        "review decisions"
    )
    print(f"Reviewer: {reviewer_id}")
    print(f"Review date: {review_date}")
    print()

    for status in sorted(counts):
        print(
            f"{status}: {counts[status]}"
        )

    print()
    print(f"Updated:")
    print(DECISIONS)
    print()


def usage():
    print(
        """
Kurmancî vocabulary review helper

EXPORT

  python3 kurmanci-review-batch.py export START END

Examples:

  python3 kurmanci-review-batch.py export 1 100
  python3 kurmanci-review-batch.py export 101 200
  python3 kurmanci-review-batch.py export 201 300


IMPORT

  REVIEWER_ID=ferhatguneri \
  python3 kurmanci-review-batch.py import WORKSHEET.md


Allowed decisions:

  approved
  approved_with_metadata_change
  rejected_from_default_pack
  experimental_only
  needs_linguist
  needs_source_investigation


Accepted aliases:

  rejected
  needs_linguists
  needs-linguist
  needs-linguists
""".strip()
    )


def main():
    if len(sys.argv) < 2:
        usage()
        raise SystemExit(2)

    command = sys.argv[1].lower()

    if command == "export":
        if len(sys.argv) != 4:
            usage()
            raise SystemExit(2)

        try:
            start = int(sys.argv[2])
            end = int(sys.argv[3])
        except ValueError:
            die(
                "START and END must be integers"
            )

        export_batch(
            start,
            end,
        )

        return

    if command == "import":
        if len(sys.argv) != 3:
            usage()
            raise SystemExit(2)

        import_batch(
            Path(sys.argv[2])
        )

        return

    usage()
    raise SystemExit(2)


if __name__ == "__main__":
    main()
