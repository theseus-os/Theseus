// We don't have a C library or anything, so we can't really call or do anything yet. 
// We just disable interrupts and write a value to a register so that we have a chance to observe
// the effects of this program running.

// create some named .rodata
const char *const HELLO = "hello world";

// create some named .data and unnamed .rodata
char *s = "me";

void test() {
	char *t = s;
	__asm__("mov %0, %%r11" : : "r"(s) : "%r11" );
	while (*t != 0) {
		t--;
	}
	__asm__("mov $0x4444555566667777, %r10");
}

int main() {
	__asm__("cli");
	__asm__("mov $0xBEEFBEEFBEEFBEEF, %r9");
	while (1) {
		test();
	}
	return 0xDEADDEAD;
}
