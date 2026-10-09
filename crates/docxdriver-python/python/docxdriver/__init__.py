"""Python SDK for the docxdriver document engine."""
import json
from ._core import __version__, canonical_plan_json, execute_request


def run_request(data, request):
    """Run the Rust typed Request API, returning metadata and optional output bytes."""
    metadata, output = execute_request(data, json.dumps(request, ensure_ascii=False, allow_nan=False))
    result = json.loads(metadata)
    if output is not None:
        result['bytes'] = output
    return result


def run_command(data, kind, **arguments):
    return run_request(data, {'Command': {'command': {'kind': kind, **arguments}}})


def run_plan(data, plan, preview_key=None):
    request = {'plan': plan}
    if preview_key is not None:
        request['preview_key'] = preview_key
    return run_request(data, {'Plan': request})
