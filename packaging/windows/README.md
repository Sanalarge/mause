# Windows setup

## Performans göstergeleri ve USB

Ayarlar → **Performans göstergeleri** bölümünde bu PC'nin toplam kullanım
verilerini paylaşma ve bağlı PC'lerin verilerini gösterme ayrı ayrı açılır.
İkisi de varsayılan olarak kapalıdır. İşlemci, RAM, GPU, disk ve ağ ikonları
ayrı ayrı seçilebilir; ikonların üzerine gelince kullanım ve aktarım hızları
görünür. Veriler yaklaşık iki saniyede bir şifreli bağlantı üzerinden gönderilir.
GPU değeri birden fazla GPU varsa en yoğun GPU'nun kullanımıdır; bellek ve
disk değerleri toplamdır. Ağ değerleri anlık trafik hızlarıdır, internet hız
testi değildir. Desteklenmeyen veya eski ölçümler sayı yerine ‘—’ gösterir.
Küçük cihaz kartlarında ikon satırı yer yoksa gösterilmez.

USB aktarımında iki PC de güncel Mause kullanmalıdır. Yeni USB tüneli TCP
24808 kullanır; kamera akışı 24807'de kalır. Windows istemcisinin yerel ucu
boş bir loopback portu seçer, böylece usbipd servisinin 3240 portuyla çakışmaz.
Takma hataları ana PC bağlantısını düşürmez ve başarısız takma işlemi arka
planda sürekli tekrarlanmaz. Cihaz ancak istemci takmayı doğrulayınca takılı
gösterilir. Kaynak PC'deki cihaz listeleme ve hazırlama işlemleri ana bağlantı
döngüsünü bekletmez.

## Ek monitör ve mevcut ekran

İki PC'ye de güncel Mause kurulmalıdır. Masaüstü genişletilecek Windows PC'de
Ayarlar → Ekran paylaşımı → **Sanal ekran sürücüsünü kur** seçilir. Bu bir kerelik
işlem internet ve yönetici onayı ister; SHA256 özeti sabitlenmiş, SignPath imzalı
Virtual Display Driver 25.7.23 paketini resmi projesinden indirir. Mevcut sürücü
ve ayarları korunur; imza denetimi veya test-signing ayarları değiştirilmez.

Diğer PC'de Ekran izleme penceresinden **Ek monitör** ve çözünürlük seçilir.
Windows masaüstü sanal ekrana genişler; pencereler bu ekrana taşınabilir.
**Tam ekran** düğmesi izleme penceresini monitör gibi kullanmayı sağlar.
**Mevcut ekranı izle** seçeneği ve ekran listesi ana/fiziksel ekranı gösterir.
Modlar paylaşım sırasında değiştirilebilir. Paylaşım kapanınca önceki ekran
düzeni geri yüklenir. Mause zorla sonlandırılırsa Windows görüntü ayarlarından
sanal ekranı kapatmak gerekebilir. Sanal ekranı Windows'un ana ekranı yapmak
desteklenmez; fiziksel ana ekran korunur.

Sürücü isteğe bağlıdır; standart Mause kurulum paketine gömülmez. Kaynak kodu
ve MIT lisansı: https://github.com/VirtualDrivers/Virtual-Display-Driver

Windows dağıtımının ana çıktısı `mause-v<sürüm>-windows-x86_64-setup.exe` dosyasıdır. Windows 10 1903+ / Windows 11 x64 içindir; ARM64 kurulum paketi bu akışta üretilmez.

## Kullanıcı kurulumu

- Yönetici izniyle Program Files altına Mause kurulur. Başlat menüsü kısayolu, isteğe bağlı masaüstü kısayolu ve Uygulamalar bölümünde kaldırma kaydı oluşturulur.
- Uygulama dosyaları kurulmadan önce gerekli Visual C++ v14 x64 çalışma zamanı kontrol edilir. Gömülü sürümden eskiyse veya eksikse kurulur.
- Varsayılan tam kurulum USB paylaşımı için usbipd-win sunucusunu ve USBip istemci sürücülerini de kurar. USB istemeyen kullanıcı kompakt kurulumu seçebilir. Bu bileşenler Windows'un paylaşılan bileşenleridir; Mause kaldırılırken başka uygulamaların kullanabileceği çalışma zamanı ve sürücüler silinmez.
- Yeniden başlatma gerekiyorsa sonunda bildirilir. Zorla yeniden başlatılmaz; yeniden başlatma beklenirken uygulama otomatik açılmaz.
- Seçilirse yalnız Mause programına özel, özel ağ profiliyle sınırlı TCP/UDP firewall kuralları eklenir. Kaldırılırken bu kurallar da silinir. USB paketlerinin kendi servis/firewall davranışı upstream paketlere aittir.
- Kullanıcı ayarları ve alınmış dosyalar kaldırmada korunur. Güncelleme aynı AppId ve kurulum klasörünü kullanır.

Kod imzalama sertifikası bu depoda yapılandırılmamıştır; Mause setup'ı imzasızdır. Bağımlılıkların SHA256 özetleri sabittir; Microsoft çalışma zamanının Authenticode imzası ve yayıncısı ayrıca doğrulanır. Bilinmeyen veya değiştirilmiş bir bağımlılık derlemeyi durdurur. Windows imza denetimi, Secure Boot ve test-signing ayarları değiştirilmez.

## Yerel üretim

Gereken geliştirme araçları: Windows x64, PowerShell, Rust/MSVC araçları. Bunlar son kullanıcının bilgisayarına kurulmaz. Fontlar ve uygulama kütüphaneleri Mause ikilisine gömülüdür; .NET veya Rust son kullanıcı gereksinimi değildir.

```powershell
./packaging/windows/Build-Installer.ps1
# Güncel release ikilisi zaten derlendiyse:
./packaging/windows/Build-Installer.ps1 -SkipBuild
```

İlk paketleme sabitlenmiş Inno Setup 6.6.1 derleyicisini, çalışma zamanını ve USB paketlerini resmi adreslerden indirir. Derleyici yalnız `target/windows-installer/tools` altına kurulur; kendi upstream lisans koşulları geçerlidir. İndirmeler `dependencies.json` özetleriyle doğrulanır. Son kullanıcı setup'ı tüm paketleri içerir ve çevrimdışı çalışır.

Çıktılar `target/windows-installer/output` altındadır:

- `*-setup.exe`: kurulum paketi.
- `*-setup.exe.sha256`: paket özeti.
- `*-windows-dependencies.json`: kullanılan derleyici/bağımlılık sürüm ve özetleri.
- `-LegacyZip` verildiyse eski güncelleyiciler için ZIP.

Bağımlılık güncellerken resmi upstream sürümü, SHA256, Microsoft imzası ve `.iss` içindeki minimum araç sürümleri birlikte gözden geçirilir. Özet hatası sonrasında doğrulamayı devre dışı bırakmayın.

## CI/CD

`ci.yml` içindeki `windows-installer` işi PR, manuel ve release çağrılarında paket üretir; temiz Windows runner'da temel kurulumu, yeniden kurulumu/güncellemeyi, uygulamanın yüklenmesini ve kaldırmayı test eder. Kaynak testleri diğer CI işlerinde çalışır. Kurulum paketi ve loglar artifact olarak tutulur. USB çekirdek sürücülerinin donanım üzerinde çalışması bu smoke testinin kapsamı değildir; CI paylaşılan runner'a USB sürücüsü kurmaz.

`release.yml` aynı betikle sürüm numarası verilmiş setup'ı ve SHA256/bağımlılık kayıtlarını üretip başarılı testlerden sonra GitHub Release'e ekler. Push/Release bu değişiklik hazırlanırken çalıştırılmadı; ilk uzak CI koşusu ayrıca izlenmelidir.

Yeni Windows güncelleyicisi setup'ı ZIP'e tercih eder, indirilen setup'ın yayınlanan SHA256 özetini doğrular ve kurucuyu açıp çalışan uygulamayı kapatır. Kurucu başarılı olursa Mause'u yeniden başlatır; yeniden başlatma gerekiyorsa açılışı erteler. Eski sürümler yalnız ZIP okuyabildiği için geçiş süresince ZIP çıktısı da korunur. Kullanıcı yeni kurulumlarda setup'ı seçmelidir.

Smoke testi yerelde sistem kurulumu yapacağından açıkça izin verilmeden çalışmaz:

```powershell
# Temiz bir test VM'sinde; mevcut Mause setup kurulumu varsa betik durur.
./packaging/windows/Test-Installer.ps1 -Setup <setup-yolu> -AllowLocalInstallation
```

Yerel doğrulama: release ve setup derlendi; güncelleyicinin setup tercihi, eski ZIP uyumluluğu ve sürüm karşılaştırması testleri geçti. PowerShell betikleri, workflow YAML ve format kontrolü doğrulandı. Genel Windows test koşusunda bu değişiklikte düzenlenmeyen ağ/tünel/DHT testlerinde hatalar görüldü; `deadline_terminates_an_incomplete_packet` takıldığı için koşu durduruldu. Bu nedenle genel testlerin veya GitHub CI'nin tamamı başarılı sayılmamalıdır. Gerçek kurulum/güncelleme/kaldırma smoke testi CI koşusunu bekler; USB sürücüleri bu bilgisayara kurulmadı.

Lisanslar ve USB sunucusunun sabit sürüm kaynak arşivi `licenses` klasörüne kurulur. Ayrıntılar: [THIRD_PARTY.md](THIRD_PARTY.md).
