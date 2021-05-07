// We don't yet have C library header files, so we can't really call or do anything yet. 

// create some named .rodata
const char *const HELLO = "hello world";

// create some named .data and unnamed .rodata
char *s = "me";

void test() {
	char *t = s;
	__asm__("mov %0, %%r11" : : "r"(s) : "%r11" );
	// while (*t != 0) {
	// 	t--;
	// }
	__asm__("mov $0x4444555566667777, %r10");
}

int main() {
	// __asm__("cli");
	// __asm__("mov $0xBEEFBEEFBEEFBEEF, %r9");
	// while (1) {
	// 	test();
	// }
	return 0x1234BEEF;
}
