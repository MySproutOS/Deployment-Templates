package main

import (
	"context"
	"os"
	"testing"

	"golang.org/x/crypto/bcrypt"
	"google.golang.org/protobuf/proto"

	"github.com/usememos/memos/internal/profile"
	"github.com/usememos/memos/internal/version"
	storepb "github.com/usememos/memos/proto/gen/store"
	"github.com/usememos/memos/store"
	"github.com/usememos/memos/store/db"
)

func TestValidatedAdminPasswordFailsClosed(t *testing.T) {
	tests := []struct {
		name  string
		value string
		valid bool
	}{
		{name: "missing", value: ""},
		{name: "short", value: "short"},
		{name: "known default", value: "passwordpasswordpasswordpassword"},
		{name: "repeated", value: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
		{name: "surrounding whitespace", value: " secure-generated-password-value-1234 "},
		{name: "generated shape", value: "nNQSwB7dL8kQh_yF3Y0jM2pA9xR6cV1zE4uI5oT7wXk", valid: true},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			t.Setenv("MEMOS_ADMIN_PASSWORD", test.value)
			_, err := validatedAdminPassword()
			if test.valid && err != nil {
				t.Fatalf("valid generated password rejected: %v", err)
			}
			if !test.valid && err == nil {
				t.Fatal("unsafe password accepted")
			}
		})
	}

	os.Unsetenv("MEMOS_ADMIN_PASSWORD")
	if _, err := validatedAdminPassword(); err == nil {
		t.Fatal("missing MEMOS_ADMIN_PASSWORD accepted")
	}
}

func TestBootstrapOwnershipIsIdempotentAndPreservesOwnerChanges(t *testing.T) {
	ctx := context.Background()
	storeInstance := newMigrationTestStore(t)
	initialPassword := "initial-generated-password-value-1234567890"
	if err := bootstrapOwnership(ctx, storeInstance, initialPassword); err != nil {
		t.Fatalf("initial bootstrap failed: %v", err)
	}

	admin, err := storeInstance.GetUser(ctx, &store.FindUser{Username: pointer(adminUsername)})
	if err != nil || admin == nil {
		t.Fatalf("seeded admin missing: user=%v error=%v", admin, err)
	}
	if admin.Role != store.RoleAdmin {
		t.Fatalf("seeded role = %q, want ADMIN", admin.Role)
	}
	if admin.Nickname != "Administrator" {
		t.Fatalf("bootstrap nickname = %q, want Administrator", admin.Nickname)
	}
	if err := bcrypt.CompareHashAndPassword([]byte(admin.PasswordHash), []byte(initialPassword)); err != nil {
		t.Fatalf("generated password does not authenticate: %v", err)
	}
	users, err := storeInstance.ListUsers(ctx, &store.FindUser{})
	if err != nil || len(users) != 1 {
		t.Fatalf("bootstrap users = %d, error=%v; want exactly one", len(users), err)
	}
	general, err := storeInstance.GetInstanceGeneralSetting(ctx)
	if err != nil || !general.DisallowUserRegistration {
		t.Fatalf("public registration was not disabled: setting=%v error=%v", general, err)
	}

	ownerPassword := "owner-changed-password-value-123456789012"
	ownerHash, err := bcrypt.GenerateFromPassword([]byte(ownerPassword), bcrypt.DefaultCost)
	if err != nil {
		t.Fatal(err)
	}
	ownerHashString := string(ownerHash)
	if _, err := storeInstance.UpdateUser(ctx, &store.UpdateUser{ID: admin.ID, PasswordHash: &ownerHashString}); err != nil {
		t.Fatalf("owner password update failed: %v", err)
	}
	general = proto.Clone(general).(*storepb.InstanceGeneralSetting)
	general.DisallowUserRegistration = false
	if _, err := storeInstance.UpsertInstanceGeneralSettingSafely(ctx, &storepb.InstanceSetting{
		Key:   storepb.InstanceSettingKey_GENERAL,
		Value: &storepb.InstanceSetting_GeneralSetting{GeneralSetting: general},
	}); err != nil {
		t.Fatalf("owner registration opt-in failed: %v", err)
	}

	if err := bootstrapOwnership(ctx, storeInstance, "different-generated-password-value-123456"); err != nil {
		t.Fatalf("second bootstrap failed: %v", err)
	}
	admin, err = storeInstance.GetUser(ctx, &store.FindUser{Username: pointer(adminUsername)})
	if err != nil || admin == nil {
		t.Fatalf("admin missing after second bootstrap: user=%v error=%v", admin, err)
	}
	if err := bcrypt.CompareHashAndPassword([]byte(admin.PasswordHash), []byte(ownerPassword)); err != nil {
		t.Fatalf("second bootstrap reset the owner password: %v", err)
	}
	general, err = storeInstance.GetInstanceGeneralSetting(ctx)
	if err != nil || general.DisallowUserRegistration {
		t.Fatalf("second bootstrap reset the owner's registration choice: setting=%v error=%v", general, err)
	}
	users, err = storeInstance.ListUsers(ctx, &store.FindUser{})
	if err != nil || len(users) != 1 {
		t.Fatalf("second bootstrap users = %d, error=%v; want exactly one", len(users), err)
	}
}

func TestBootstrapOwnershipRecoversPendingRegistrationLockdown(t *testing.T) {
	ctx := context.Background()
	storeInstance := newMigrationTestStore(t)
	password := "initial-generated-password-value-1234567890"
	passwordHash, err := bcrypt.GenerateFromPassword([]byte(password), bcrypt.DefaultCost)
	if err != nil {
		t.Fatal(err)
	}
	seededAdmin, created, err := storeInstance.CreateUserIfNoUsers(ctx, &store.User{
		Username:     adminUsername,
		Role:         store.RoleAdmin,
		Nickname:     adminBootstrapMarker,
		PasswordHash: string(passwordHash),
	})
	if err != nil || !created {
		t.Fatalf("create pending admin: created=%v error=%v", created, err)
	}

	if err := bootstrapOwnership(ctx, storeInstance, "different-generated-password-value-123456"); err != nil {
		t.Fatalf("resume bootstrap failed: %v", err)
	}
	seededAdmin, err = storeInstance.GetUser(ctx, &store.FindUser{Username: pointer(adminUsername)})
	if err != nil || seededAdmin == nil {
		t.Fatalf("recovered admin missing: user=%v error=%v", seededAdmin, err)
	}
	if seededAdmin.Nickname != "Administrator" {
		t.Fatalf("bootstrap nickname after recovery = %q, want Administrator", seededAdmin.Nickname)
	}
	if err := bcrypt.CompareHashAndPassword([]byte(seededAdmin.PasswordHash), []byte(password)); err != nil {
		t.Fatalf("recovery reset the pending administrator password: %v", err)
	}
	general, err := storeInstance.GetInstanceGeneralSetting(ctx)
	if err != nil || !general.DisallowUserRegistration {
		t.Fatalf("recovery did not disable public registration: setting=%v error=%v", general, err)
	}
}

func newMigrationTestStore(t *testing.T) *store.Store {
	t.Helper()
	instanceProfile := &profile.Profile{
		Driver:  "sqlite",
		Data:    t.TempDir(),
		Version: version.GetCurrentVersion(),
		Commit:  version.Commit,
	}
	if err := instanceProfile.Validate(); err != nil {
		t.Fatal(err)
	}
	driver, err := db.NewDBDriver(instanceProfile)
	if err != nil {
		t.Fatal(err)
	}
	storeInstance := store.New(driver, instanceProfile)
	if err := storeInstance.Migrate(context.Background()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := storeInstance.Close(); err != nil {
			t.Errorf("close store: %v", err)
		}
	})
	return storeInstance
}
