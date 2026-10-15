// Open XML SDK validation for scripts/ooxml-conformance.py.
//
// Reads one .docx path per stdin line and writes one JSON line per path:
// {"path": ..., "errors": [{"part", "id", "description", "xpath"}]} or
// {"path": ..., "fatal": ...} when the SDK cannot open the package. Unlike the
// ECMA-376 XSDs, the SDK validates the Office extension namespaces (w14, w15,
// w16cid, w16cex, ...) and a set of semantic constraints, against the
// Microsoft 365 file format.
using System.Text.Json;
using DocumentFormat.OpenXml;
using DocumentFormat.OpenXml.Packaging;
using DocumentFormat.OpenXml.Validation;

var validator = new OpenXmlValidator(FileFormatVersions.Microsoft365) { MaxNumberOfErrors = 5000 };
string? path;
while ((path = Console.In.ReadLine()) != null)
{
    if (path.Length == 0) continue;
    object result;
    try
    {
        using var document = WordprocessingDocument.Open(path, false);
        var errors = validator.Validate(document).Select(error => new
        {
            part = error.Part?.Uri.ToString() ?? "",
            id = error.Id ?? "",
            description = error.Description ?? "",
            xpath = error.Path?.XPath ?? "",
        }).ToList();
        result = new { path, errors };
    }
    catch (Exception exception)
    {
        result = new { path, fatal = $"{exception.GetType().Name}: {exception.Message}" };
    }
    Console.Out.WriteLine(JsonSerializer.Serialize(result));
    Console.Out.Flush();
}
