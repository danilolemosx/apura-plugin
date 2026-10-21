#!/bin/sh
# Monta o instalador do Mac (Apura-<versão>-mac.pkg) a partir dos plugins compilados.
#
#   scripts/empacotar-mac.sh <pasta com Apura.vst3, Apura.component e o .aaxplugin> <versão>
#
# O AAX é assinado aqui com as ferramentas da PACE: precisa do iLok de assinatura conectado e do
# iLok License Manager aberto na conta de assinatura. Os dados da assinatura vêm de variáveis ou do
# arquivo local scripts/assinatura.local (fora do repositório):
#   PACE_CUSTOMER_NUMBER, PACE_CUSTOMER_NAME, SIGN_ID (certificado de assinatura do Mac)
#
# O instalador não é assinado pela Apple: na primeira vez, o macOS pede "Abrir Mesmo Assim" em
# Ajustes do Sistema → Privacidade e Segurança. O que ele instala já fica liberado (sem quarentena).
set -e
# sem arquivos "._" (metadados do macOS) dentro do pacote
export COPYFILE_DISABLE=1
cd "$(dirname "$0")/.."
ORIGEM="$1"
VERSAO="$2"
[ -d "$ORIGEM" ] && [ -n "$VERSAO" ] || { echo "uso: $0 <pasta dos plugins> <versão>"; exit 1; }

WRAPTOOL=/Applications/PACEAntiPiracy/Eden/Fusion/Versions/6/bin/wraptool
[ -f scripts/assinatura.local ] && . scripts/assinatura.local
[ -n "$PACE_CUSTOMER_NUMBER" ] && [ -n "$PACE_CUSTOMER_NAME" ] && [ -n "$SIGN_ID" ] || {
	echo "defina PACE_CUSTOMER_NUMBER, PACE_CUSTOMER_NAME e SIGN_ID (ou scripts/assinatura.local)"; exit 1; }

AAX_ORIGEM=$(ls -d "$ORIGEM"/Apura.aaxplugin* | head -1)
for f in "$ORIGEM/Apura.vst3" "$ORIGEM/Apura.component" "$AAX_ORIGEM"; do
	[ -d "$f" ] || { echo "faltando: $f"; exit 1; }
done

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
RAIZ="$TMP/raiz"
mkdir -p "$RAIZ/Library/Audio/Plug-Ins/VST3" "$RAIZ/Library/Audio/Plug-Ins/Components" \
         "$RAIZ/Library/Application Support/Avid/Audio/Plug-Ins"
ditto "$ORIGEM/Apura.vst3" "$RAIZ/Library/Audio/Plug-Ins/VST3/Apura.vst3"
ditto "$ORIGEM/Apura.component" "$RAIZ/Library/Audio/Plug-Ins/Components/Apura.component"

echo "== assinando VST3 e AU (certificado do Mac)"
# o JUCE grava o moduleinfo.json do VST3 depois de assinar, e o AU universal vem sem assinatura
for p in "$RAIZ/Library/Audio/Plug-Ins/VST3/Apura.vst3" "$RAIZ/Library/Audio/Plug-Ins/Components/Apura.component"; do
	codesign --force --sign "$SIGN_ID" --timestamp=none "$p"
	codesign --verify --strict "$p"
done

echo "== assinando o AAX (PACE)"
ditto "$AAX_ORIGEM" "$TMP/Apura.aaxplugin"
"$WRAPTOOL" sign --customernumber "$PACE_CUSTOMER_NUMBER" --customername "$PACE_CUSTOMER_NAME" \
	--productname "Apura" --signid "$SIGN_ID" \
	--in "$TMP/Apura.aaxplugin" --out "$RAIZ/Library/Application Support/Avid/Audio/Plug-Ins/Apura.aaxplugin"
"$WRAPTOOL" verify --in "$RAIZ/Library/Application Support/Avid/Audio/Plug-Ins/Apura.aaxplugin" | grep -q "The digital signature was verified"

echo "== instalador"
xattr -cr "$RAIZ"
pkgbuild --root "$RAIZ" --identifier br.danilolemos.apura.plugins --version "$VERSAO" \
	--install-location / "$TMP/apura-plugins.pkg"
mkdir -p "$TMP/recursos"
cp scripts/instalador/*.html "$TMP/recursos/"
cat > "$TMP/distribution.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>Apura $VERSAO</title>
    <welcome file="bem-vindo.html" mime-type="text/html"/>
    <license file="licenca.html" mime-type="text/html"/>
    <conclusion file="concluido.html" mime-type="text/html"/>
    <options customize="never" require-scripts="false" hostArchitectures="arm64,x86_64"/>
    <domains enable_localSystem="true"/>
    <choices-outline><line choice="apura"/></choices-outline>
    <choice id="apura" title="Apura (VST3, AU e AAX)">
        <pkg-ref id="br.danilolemos.apura.plugins"/>
    </choice>
    <pkg-ref id="br.danilolemos.apura.plugins" version="$VERSAO">apura-plugins.pkg</pkg-ref>
</installer-gui-script>
EOF
mkdir -p dist
productbuild --distribution "$TMP/distribution.xml" --resources "$TMP/recursos" --package-path "$TMP" \
	"dist/Apura-$VERSAO-mac.pkg"
echo "pronto: dist/Apura-$VERSAO-mac.pkg"
