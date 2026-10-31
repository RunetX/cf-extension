// Read-only adapter for local ICU ResB v2 resources. No platform assets are shipped.
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import * as crypto from 'node:crypto';
import { inflateRawSync } from 'node:zlib';
const LIMIT = 16 * 1024 * 1024;
export function resource(bundle: Buffer, name: string): Buffer | undefined {
    if (bundle.length < 24 || bundle[2] !== 0xda || bundle[3] !== 0x27 || bundle.toString('ascii', 12, 16) !== 'ResB' || bundle[8] !== 0 || bundle[16] !== 2) throw new Error('Неподдерживаемый пакет ICU ResB v2');
    const header = bundle.readUInt16LE(0);
    if (header < 24 || header + 32 > bundle.length) throw new Error('Повреждённый заголовок ресурсов');
    const data = bundle.subarray(header), root = data.readUInt32LE(0), type = root >>> 28, at = (root & 0x0fffffff) * 4;
    const keysTop = data.readUInt32LE(8) * 4;
    if (keysTop > data.length || (data.readUInt32LE(24) & 4)) throw new Error('Пул ключей не поддерживается');
    if (type !== 2 && type !== 4) throw new Error('Корень ресурсов не является таблицей');
    const count = type === 2 ? data.readUInt16LE(at) : data.readUInt32LE(at);
    if (count > 100000) throw new Error('Слишком много ресурсов');
    const keyBase = at + (type === 2 ? 2 : 4), keySize = type === 2 ? 2 : 4;
    const valueBase = type === 2 ? (keyBase + count * 2 + 3) & ~3 : keyBase + count * 4;
    if (valueBase + count * 4 > data.length) throw new Error('Таблица за пределами файла');
    for (let i = 0; i < count; i++) {
        const offset = keySize === 2 ? data.readUInt16LE(keyBase + i * 2) : data.readUInt32LE(keyBase + i * 4);
        if (offset >= keysTop) throw new Error('Некорректный ключ');
        const keyBytes = data.subarray(offset, Math.min(offset + 512, keysTop));
        const keyLength = keyBytes.indexOf(0);
        const end = offset + keyLength;
        if (keyLength < 0 || end >= keysTop) throw new Error('Незавершённый ключ');
        if (data.toString('utf8', offset, end) !== name) continue;
        const ref = data.readUInt32LE(valueBase + i * 4);
        if ((ref >>> 28) !== 1) return undefined;
        const start = (ref & 0x0fffffff) * 4;
        const length = data.readUInt32LE(start);
        if (length > LIMIT || start + 4 + length > data.length) throw new Error('Некорректный размер ресурса');
        return data.subarray(start + 4, start + 4 + length);
    }
    return undefined;
}
export function zipMember(zip: Buffer, names: string[]): Buffer | undefined {
    const end = zip.lastIndexOf(Buffer.from([0x50,0x4b,0x05,0x06]));
    if (end < 0 || end + 22 > zip.length) throw new Error('Нет каталога ZIP');
    let at = zip.readUInt32LE(end + 16);
    const count = zip.readUInt16LE(end + 10);
    for (let i = 0; i < count; i++) {
        if (at + 46 > zip.length || zip.readUInt32LE(at) !== 0x02014b50) throw new Error('Повреждён ZIP');
        const size = zip.readUInt32LE(at+20), unpacked = zip.readUInt32LE(at+24), nameSize = zip.readUInt16LE(at+28);
        const next = at + 46 + nameSize + zip.readUInt16LE(at+30) + zip.readUInt16LE(at+32);
        if (next > zip.length) throw new Error('Повреждён ZIP entry');
        const name = zip.toString('utf8',at+46,at+46+nameSize);
        if (names.includes(name)) {
            if ((zip.readUInt16LE(at+8)&1) || size > LIMIT || unpacked > LIMIT) throw new Error('Ресурс ZIP за пределами ограничений');
            const local = zip.readUInt32LE(at+42), method = zip.readUInt16LE(at+10);
            if (local + 30 > zip.length || zip.readUInt32LE(local) !== 0x04034b50) throw new Error('Некорректный локальный ZIP header');
            const start = local + 30 + zip.readUInt16LE(local+26) + zip.readUInt16LE(local+28);
            if (start+size > zip.length) throw new Error('ZIP данные за концом файла');
            const bytes = zip.subarray(start,start+size);
            const result = method === 0 ? bytes : method === 8 ? inflateRawSync(bytes,{maxOutputLength:LIMIT}) : undefined;
            if (!result || result.length !== unpacked) throw new Error('Неподдерживаемое сжатие ZIP');
            return result;
        }
        at = next;
    }
    return undefined;
}
const STRIPS: Array<[string,string,string]> = [
    ['backend','MainMDImages.zip','MainMDImages.png'], ['basic','BasicMDImages.zip','BasicMDImages.png'],
    ['calc','CalcMDImages.zip','CalcMDImages.png'], ['accnt','TbAccntUIMD.zip','TbAccntUIMD.png'],
    ['bp','BusinessProcess.zip','BusinessProcess.png'], ['backend','CommonAttrubuteImages.png',''],
    ['backend','bot.png',''], ['backend','web_socket_client.png',''], ['backend','palette.png',''],
    ['basic','TbBasicUIMD.zip','TbBasicUIMD.png'], ['bp','Images.zip','l.png'],
];
const GLYPHS: Record<string,[number,number,number?]> = {
    Configuration:[0,0], Common:[0,23], Language:[0,1], Subsystem:[0,3], Style:[0,5], StyleItem:[0,7],
    CommonPicture:[0,9], Template:[0,11], CommonTemplate:[0,13], Role:[0,17], TabularSection:[0,19],
    Attribute:[0,21], CommonAttribute:[5,4], Bot:[6,0], WebSocketClient:[7,0], PaletteColor:[8,0],
    FilterCriterion:[0,24], CommonModule:[0,28], ExchangePlan:[0,30], SessionParameter:[0,35],
    XDTOPackage:[0,37], WebService:[0,39], WSReference:[0,45], CommonCommand:[0,49], Command:[0,49],
    CommandGroup:[0,51], EventSubscription:[0,53], ScheduledJob:[0,58], AddressingAttribute:[0,62],
    SettingsStorage:[0,65,64], FunctionalOption:[0,66], FunctionalOptionsParameter:[0,68], DefinedType:[0,72,71],
    HTTPService:[0,73], URLTemplate:[0,74], IntegrationService:[0,76], Constant:[1,0], Catalog:[1,2],
    Form:[1,6], CommonForm:[1,7,6], Enum:[1,8], EnumValue:[1,10], Document:[1,12], DocumentJournal:[1,14],
    Column:[1,16], DataProcessor:[1,20], ExternalDataProcessor:[1,20], DocumentNumerator:[1,27],
    Report:[1,29], ExternalReport:[1,29], Dimension:[1,31], Resource:[1,33], Sequence:[1,36,35],
    AccumulationRegister:[1,37], InformationRegister:[1,39], ChartOfCharacteristicTypes:[9,5],
    ChartOfCalculationTypes:[2,1,0], CalculationRegister:[2,8], ChartOfAccounts:[3,6,0],
    AccountingRegister:[3,1], AccountingFlag:[3,2], ExtDimensionAccountingFlag:[3,3], BusinessProcess:[4,4], Task:[10,2],
};
export interface IconFiles { light: string; dark: string; }
export interface PlatformIconSet { files: Map<string,IconFiles>; directory?: string; diagnostics: string[]; }
export async function discoverPlatform(configured?: string): Promise<string | undefined> {
    if (configured) {
        for (const dir of [configured,path.join(configured,'bin')]) { try { await fs.access(path.join(dir,'backend_root.res')); return dir; } catch { /* try next */ } }
        throw new Error('В указанном каталоге нет backend_root.res; укажите bin установленной 1С');
    }
    if (process.env.CFKIT_PLATFORM) return discoverPlatform(process.env.CFKIT_PLATFORM);
    const roots = process.platform === 'win32'
        ? [process.env.ProgramFiles,process.env['ProgramFiles(x86)']].filter((p):p is string=>!!p).map(p=>path.join(p,'1cv8'))
        : ['/opt/1cv8/x86_64','/opt/1cv8'];
    for (const root of roots) {
        let versions: string[]; try { versions = (await fs.readdir(root)).filter(n=>/^\d+(\.\d+)+$/.test(n)).sort((a,b)=>b.localeCompare(a,undefined,{numeric:true})); } catch { continue; }
        for (const version of versions) { const dir=path.join(root,version,'bin'); try { await fs.access(path.join(dir,'backend_root.res')); return dir; } catch { /* try next */ } }
    }
    return undefined;
}
export async function loadPlatformIcons(directory: string | undefined, cache: string): Promise<PlatformIconSet> {
    const result: PlatformIconSet = {files:new Map(),directory,diagnostics:[]};
    if (!directory) return result;
    const bundles=new Map<string,Buffer>(); const strips=new Map<number,Buffer>();
    for (let i=0;i<STRIPS.length;i++) {
        const [module,key,member]=STRIPS[i];
        try {
            let bundle=bundles.get(module);
            if (!bundle) { const file=path.join(directory,module+'_root.res'); const stat=await fs.stat(file); if(stat.size>128*1024*1024)throw new Error('Слишком большой ресурсный пакет'); bundle=await fs.readFile(file); bundles.set(module,bundle); }
            const bytes=resource(bundle,key); if(!bytes)continue;
            const png=member?zipMember(bytes,[member,member.replace('.png','_100.png')]):bytes;
            if(png)strips.set(i,png);
        } catch(error) { result.diagnostics.push(`${key}: ${String(error)}`); }
    }
    await fs.mkdir(cache,{recursive:true});
    for (const [kind,[strip,index,groupIndex]] of Object.entries(GLYPHS)) {
        for(const group of [false,true]) {
            const actualIndex=group && groupIndex!==undefined?groupIndex:index;
            // Characteristic type groups use the original basic strip, objects use its UI strip.
            const png=strips.get(kind==='ChartOfCharacteristicTypes'&&group?1:strip);
            const glyph=kind==='ChartOfCharacteristicTypes'&&group?43:actualIndex;
            if(!png || png.length<33 || !png.subarray(0,8).equals(Buffer.from([137,80,78,71,13,10,26,10])))continue;
            const width=png.readUInt32BE(16),height=png.readUInt32BE(20);
            if(height!==16 || width<16*(glyph+1) || width>16384)continue;
            const digest=crypto.createHash('sha256').update(png).update(`${glyph}`).digest('hex').slice(0,24);
            const files:IconFiles={light:path.join(cache,digest+'.svg'),dark:path.join(cache,digest+'-dark.svg')};
            for (const dark of [false,true]) {
                const filter=dark?'<defs><filter id="halo" x="-10%" y="-30%" width="120%" height="160%"><feDropShadow dx="0" dy="0" stdDeviation="0.35" flood-color="#ddd" flood-opacity="0.8"/></filter></defs>':'';
                const svg=`<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="${glyph*16} 0 16 16" overflow="hidden">${filter}<image width="${width}" height="${height}" href="data:image/png;base64,${png.toString('base64')}"${dark?' filter="url(#halo)"':''}/></svg>`;
                await fs.writeFile(dark?files.dark:files.light,svg,'utf8');
            }
            result.files.set(`${kind}:${group?'group':'object'}`,files);
        }
    }
    return result;
}
