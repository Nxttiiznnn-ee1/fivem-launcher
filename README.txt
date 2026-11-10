FiveM Launcher (Rust + egui)
===========================

วิธีสร้างไฟล์ .exe (เลือกทางใดทางหนึ่ง)

ทางที่ 1: บนเครื่อง Windows
  1. ติดตั้ง Rust จาก https://rustup.rs
     (ตัวติดตั้งจะบอกให้ลง "Visual Studio Build Tools" ด้วย ให้ลงตามที่บอก)
  2. เปิดโฟลเดอร์นี้ใน Terminal แล้วรัน:
         cargo build --release
  3. ได้ไฟล์ target\release\fivem-launcher.exe  (ไฟล์เดียว ย้ายไปไว้ที่ไหนก็ได้)

ทางที่ 2: ให้ GitHub สร้างให้ (ไม่ต้องลงอะไรในเครื่อง)
  1. สร้าง repository ใน GitHub แล้วอัปโหลดไฟล์ทั้งหมดในโฟลเดอร์นี้ (รวมโฟลเดอร์ .github)
  2. เปิดแท็บ Actions -> build-windows-exe -> Run workflow
  3. รอสักครู่ แล้วดาวน์โหลด "FiveMLauncher" จากส่วน Artifacts

รายชื่อเซิร์ฟเวอร์เก็บที่ %APPDATA%\FiveMLauncher\servers.json
(เป็นไฟล์เดียวกับเวอร์ชัน Go ที่เคยทำไว้ รายชื่อเดิมใช้ต่อได้)
