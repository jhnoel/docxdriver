#!/usr/bin/env python3
"""Build the fixed DOCX used to compare projection versions; stdlib only.

Run from any directory. An optional --baseline-reader path to the parity_read
example compiled from commit 0e5a0f9c regenerates prior-projection.json.
"""
from pathlib import Path
import argparse, base64, json, subprocess, zipfile
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'crates/docxdriver-core/tests/fixtures/projection-parity'
W='http://schemas.openxmlformats.org/wordprocessingml/2006/main'
M='http://schemas.openxmlformats.org/officeDocument/2006/math'
R='http://schemas.openxmlformats.org/officeDocument/2006/relationships'
REL='http://schemas.openxmlformats.org/package/2006/relationships'
NS=f'xmlns:w="{W}" xmlns:m="{M}" xmlns:r="{R}" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"'
def run(text,pr=''):return f'<w:r>{"<w:rPr>"+pr+"</w:rPr>" if pr else ""}<w:t xml:space="preserve">{text}</w:t></w:r>'
def para(pid,content,pr=''):return f'<w:p w14:paraId="{pid}">{"<w:pPr>"+pr+"</w:pPr>" if pr else ""}{content}</w:p>'
def mr(text,pr=''):return f'<m:r>{"<m:rPr>"+pr+"</m:rPr>" if pr else ""}<m:t>{text}</m:t></m:r>'
def e(text):return f'<m:e>{mr(text)}</m:e>'
def image(rid,width,height,alt):return f'<w:r><w:drawing><wp:inline><wp:extent cx="{width*9525}" cy="{height*9525}"/><wp:docPr id="1" name="Picture" descr="{alt}"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="{rid}"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>'
def rels(entries):return f'<Relationships xmlns="{REL}">'+''.join(f'<Relationship Id="{i}" Type="{R}/{t}" Target="{target}"{mode}/>' for i,t,target,mode in entries)+'</Relationships>'
body=para('11111111',run('Bold','<w:b/>')+run(' italic','<w:i/>')+run(' underline','<w:u w:val="single"/>')+run(' strike','<w:strike/>')+run(' super','<w:vertAlign w:val="superscript"/>')+run(' sub','<w:vertAlign w:val="subscript"/>')+'<w:hyperlink r:id="safe">'+run('Safe link')+'</w:hyperlink><w:hyperlink r:id="unsafe">'+run('Opaque link')+'</w:hyperlink><w:hyperlink w:anchor="bookmark">'+run('Internal link')+'</w:hyperlink>', '<w:pStyle w:val="CustomStyle"/>')
body+=para('22222222','<w:fldSimple w:instr="AUTHOR">'+run('Author result')+'</w:fldSimple><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> REF source </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>'+run('Complex result')+'<w:r><w:fldChar w:fldCharType="end"/></w:r><w:commentRangeStart w:id="0"/>'+run('Commented')+'<w:commentRangeEnd w:id="0"/>'+run(' changed-format','<w:b/><w:rPrChange w:id="31" w:author="Format Editor"><w:rPr/></w:rPrChange>')+run(' neutral-format','<w:b w:val="0"/><w:rPrChange w:id="32" w:author="Format Editor"><w:rPr><w:b/></w:rPr></w:rPrChange>'))
for pid,level,text in [('33333333',0,'Roman item'),('33333334',1,'Nested item'),('33333335',0,'Next item')]:body+=para(pid,run(text),f'<w:numPr><w:ilvl w:val="{level}"/><w:numId w:val="42"/></w:numPr>')
body+=para('44444444',run('Common ')+'<w:ins w:id="21" w:author="Insert Editor">'+run('inserted')+'</w:ins><w:del w:id="22" w:author="Delete Editor"><w:r><w:delText>deleted</w:delText></w:r></w:del><w:r><w:footnoteReference w:id="7"/><w:endnoteReference w:id="9"/></w:r><w:del w:id="24" w:author="Delete Editor"><w:r><w:footnoteReference w:id="10"/></w:r></w:del><w:ins w:id="25" w:author="Insert Editor"><w:r><w:endnoteReference w:id="11"/></w:r></w:ins>')
body+=para('55555555',image('available',32,24,'Available picture')+image('opaque',48,36,'Opaque picture'))
body+=para('66666666',run('Unicode 😀 nonbreaking space literal\ttab')+'<w:r><w:tab/><w:br/><w:br w:type="page"/><w:br w:type="column"/></w:r><mc:AlternateContent><mc:Choice Requires="unsupported"><w:r><w:t>Unsupported choice</w:t></w:r></mc:Choice><mc:Fallback>'+run('Fallback content')+'</mc:Fallback></mc:AlternateContent>')
body+=para('77777777','<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> REF joined </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>'+run('Joined first'),'<w:rPr><w:del w:id="23" w:author="Break Editor"/></w:rPr>')
body+=para('88888888',run('Joined second')+'<w:r><w:fldChar w:fldCharType="end"/></w:r>')
body+=para('99999999',run('Inserted break first'),'<w:rPr><w:ins w:id="26" w:author="Break Editor"/></w:rPr>')+para('99999998',run('Inserted break second'))
body+='<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge w:val="restart"/></w:tcPr>'+para('AA000001',run('Merged cell'))+'</w:tc><w:tc>'+para('AA000002',run('Top cell'))+'</w:tc></w:tr><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge/></w:tcPr><w:p/></w:tc><w:tc><w:tbl><w:tr><w:tc>'+para('AA000003',run('Nested cell'))+'</w:tc></w:tr></w:tbl><w:p/></w:tc></w:tr></w:tbl>'
formulas=[
 ('runs',mr('α + β')+mr('sin','<m:sty m:val="p"/>')),
 ('sup',f'<m:sSup>{e("x")}<m:sup>{mr("2")}</m:sup></m:sSup>'),
 ('sub',f'<m:sSub>{e("x")}<m:sub>{mr("i")}</m:sub></m:sSub>'),
 ('subsup',f'<m:sSubSup>{e("x")}<m:sub>{mr("i")}</m:sub><m:sup>{mr("2")}</m:sup></m:sSubSup>'),
 ('prescripts',f'<m:sPre>{e("X")}<m:sub>{mr("a")}</m:sub><m:sup>{mr("b")}</m:sup></m:sPre>'),
 ('fraction',f'<m:f><m:num>{mr("a")}</m:num><m:den>{mr("b")}</m:den></m:f>'),
 ('nobar',f'<m:f><m:fPr><m:type m:val="noBar"/></m:fPr><m:num>{mr("n")}</m:num><m:den>{mr("k")}</m:den></m:f>'),
 ('skew',f'<m:f><m:fPr><m:type m:val="skw"/></m:fPr><m:num>{mr("a")}</m:num><m:den>{mr("b")}</m:den></m:f>'),
 ('sqrt',f'<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/>{e("x")}</m:rad>'),
 ('root',f'<m:rad><m:deg>{mr("3")}</m:deg>{e("x")}</m:rad>'),
 ('nary',f'<m:nary><m:naryPr><m:chr m:val="∑"/><m:subHide m:val="true"/></m:naryPr><m:sub>{mr("HIDDEN")}</m:sub><m:sup>{mr("N")}</m:sup>{e("x")}</m:nary>'),
 ('limlow',f'<m:limLow>{e("lim")}<m:lim>{mr("0")}</m:lim></m:limLow>'),
 ('limupp',f'<m:limUpp>{e("max")}<m:lim>{mr("N")}</m:lim></m:limUpp>'),
 ('function',f'<m:func><m:fName>{mr("sin","<m:sty m:val=\"p\"/>")}</m:fName>{e("x")}</m:func>'),
 ('accent',f'<m:acc><m:accPr><m:chr m:val="→"/></m:accPr>{e("v")}</m:acc>'),
 ('bar',f'<m:bar>{e("x")}</m:bar>'),
 ('underbar',f'<m:bar><m:barPr><m:pos m:val="bot"/></m:barPr>{e("y")}</m:bar>'),
 ('group-default',f'<m:groupChr>{e("x")}</m:groupChr>'),
 ('underbrace',f'<m:groupChr><m:groupChrPr><m:pos m:val="bot"/><m:chr m:val="⏟"/></m:groupChrPr>{e("x")}</m:groupChr>'),
 ('delimiters',f'<m:d><m:dPr><m:begChr m:val="["/><m:endChr m:val="]"/><m:sepChr m:val=";"/></m:dPr>{e("a")}{e("b")}</m:d>'),
 ('matrix',f'<m:m><m:mr>{e("a")}{e("b")}</m:mr><m:mr>{e("c")}{e("d")}</m:mr></m:m>'),
 ('equation-array',f'<m:eqArr>{e("a=b")}{e("c=d")}</m:eqArr>'),
 ('box',f'<m:box>{e("x")}</m:box>'),
 ('borderbox',f'<m:borderBox>{e("y")}</m:borderBox>'),
 ('phantom',f'<m:phant>{e("z")}</m:phant>'),
 ('deleted-run','<m:r><m:delText>Old math</m:delText></m:r>'),
 ('unknown',f'<m:unknown>{e("Retained unknown")}</m:unknown>')]
for i,(name,formula) in enumerate(formulas):body+=para(f'F{i:07X}',run(name+': ')+f'<m:oMath>{formula}</m:oMath>')
body+=para('FFFF0001','<m:oMathPara><m:oMath>'+mr('display')+'</m:oMath></m:oMathPara>')
def sect(first=False):return '<w:sectPr>'+''.join(f'<w:{kind}Reference w:type="{slot}" r:id="{kind}-{slot}"/>' for kind in ['header','footer'] for slot in ['default','first','even'])+('<w:titlePg/>' if first else '')+'</w:sectPr>'
body+=para('BB000001',run('Section one end'),sect(True))+para('BB000002',run('Section two heading'),'<w:pStyle w:val="Heading2"/>')+sect()
parts={'word/document.xml':f'<w:document {NS}><w:body>{body}</w:body></w:document>',
 'word/styles.xml':f'<w:styles xmlns:w="{W}"><w:style w:type="paragraph" w:styleId="CustomStyle"><w:name w:val="Custom style"/></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="Heading 2"/></w:style></w:styles>',
 'word/numbering.xml':f'<w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="42"><w:lvl w:ilvl="0"><w:start w:val="3"/><w:numFmt w:val="upperRoman"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/></w:lvl></w:abstractNum><w:num w:numId="42"><w:abstractNumId w:val="42"/></w:num></w:numbering>',
 'word/footnotes.xml':f'<w:footnotes {NS}><w:footnote w:id="7">'+para('CC000007',run('Footnote body','<w:b/>'))+'<w:tbl><w:tr><w:tc>'+para('CC000008',run('Footnote table'))+'</w:tc></w:tr></w:tbl></w:footnote><w:footnote w:id="10">'+para('CC000010',run('Deleted footnote body'))+'</w:footnote></w:footnotes>',
 'word/endnotes.xml':f'<w:endnotes {NS}><w:endnote w:id="9">'+para('DD000009',run('Endnote body','<w:i/>'))+'</w:endnote><w:endnote w:id="11">'+para('DD000011',run('Inserted endnote body'))+'</w:endnote></w:endnotes>',
 'word/comments.xml':f'<w:comments {NS}><w:comment w:id="0" w:author="Comment Author">'+para('EE000001',run('Comment body'))+'</w:comment></w:comments>',
 'word/media/image.png':base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jV1kAAAAASUVORK5CYII='),
 'word/media/image.emf':b'opaque metafile payload'}
entries=[('safe','hyperlink','https://example.com/?a=1&amp;b=2',' TargetMode="External"'),('unsafe','hyperlink','custom-scheme:opaque-target',' TargetMode="External"'),('available','image','media/image.png',''),('opaque','image','media/image.emf',''),('numbering','numbering','numbering.xml',''),('styles','styles','styles.xml',''),('settings','settings','settings.xml',''),('footnotes','footnotes','footnotes.xml',''),('endnotes','endnotes','endnotes.xml',''),('comments','comments','comments.xml','')]
for kind in ['header','footer']:
 for slot in ['default','first','even']:
  filename=f'{kind}-{slot}.xml';tag='hdr' if kind=='header' else 'ftr'
  parts['word/'+filename]=f'<w:{tag} {NS}>'+para('AB000001',run(kind+' '+slot)) + f'</w:{tag}>'
  entries.append((kind+'-'+slot,kind,filename,''))
parts['word/_rels/document.xml.rels']=rels(entries)
parser=argparse.ArgumentParser();parser.add_argument('--baseline-reader');args=parser.parse_args();OUT.mkdir(parents=True,exist_ok=True)
path=OUT/'parity.docx'
with zipfile.ZipFile(ROOT/'crates/docxdriver-core/assets/blank.docx') as blank,zipfile.ZipFile(path,'w',zipfile.ZIP_DEFLATED) as output:
 def write(name,data):
  info=zipfile.ZipInfo(name,date_time=(2026,1,1,0,0,0));info.compress_type=zipfile.ZIP_DEFLATED;output.writestr(info,data)
 import xml.etree.ElementTree as ET
 ct_ns='http://schemas.openxmlformats.org/package/2006/content-types';ET.register_namespace('',ct_ns)
 types=ET.fromstring(blank.read('[Content_Types].xml'))
 for extension,mime in [('png','image/png'),('emf','image/x-emf')]:
  if not any(n.get('Extension')==extension for n in types):ET.SubElement(types,'{'+ct_ns+'}Default',Extension=extension,ContentType=mime)
 for name in parts:
  if not name.endswith('.xml') or '_rels/' in name or any(n.get('PartName')=='/'+name for n in types):continue
  kind='header' if '/header-' in name else 'footer' if '/footer-' in name else name.rsplit('/',1)[-1][:-4]
  ET.SubElement(types,'{'+ct_ns+'}Override',PartName='/'+name,ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.'+kind+'+xml')
 parts['[Content_Types].xml']=ET.tostring(types,encoding='utf-8')
 for name in blank.namelist():
  if name not in parts:write(name,blank.read(name))
 for name,data in sorted(parts.items()):write(name,data)
(OUT/'features.json').write_text(json.dumps({'baseline_commit':'0e5a0f9c559b3623bf5792e187c8c846fe627dfc','equations':[name for name,_ in formulas]+['display']},indent=2)+'\n')
if args.baseline_reader:
 result=json.loads(subprocess.check_output([args.baseline_reader,str(path)],text=True))
 (OUT/'prior-projection.json').write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n')
print(path)
