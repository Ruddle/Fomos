mkdir -p target
gcc -nostdlib -static -fPIE -pie -o ./target/main main.c